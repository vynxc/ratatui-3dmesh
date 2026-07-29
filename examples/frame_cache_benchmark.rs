use std::{
    alloc::{GlobalAlloc, Layout, System},
    collections::hash_map::DefaultHasher,
    env,
    hash::{Hash, Hasher},
    hint::black_box,
    mem::size_of,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};

use ratatui::{buffer::Buffer, layout::Rect};
use ratatui_3dmesh::{
    render::{render_prepared_mesh, PreparedMesh},
    ColorMode, FrameCacheConfig, Mesh, Mesh3dConfig, Mesh3dState, TextureFilter,
};

struct CountingAllocator;

static COUNT_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = System.alloc(layout);
        if COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
    }
}

fn main() -> ratatui_3dmesh::Result<()> {
    let args = env::args().collect::<Vec<_>>();
    let model_path = args
        .get(1)
        .map(String::as_str)
        .unwrap_or("/home/vynxc/code/v-rice/desktop-widget/assets/twins.glb");
    let width = parse_arg(&args, 2, 859_u16);
    let height = parse_arg(&args, 3, 143_u16);
    let replay_frames = parse_arg(&args, 4, 1_000_usize);
    let fps = parse_arg(&args, 5, 15_u16);
    let max_bytes = parse_arg(&args, 6, 512_usize * 1024 * 1024);
    let mode = args.get(7).map_or("all", String::as_str);

    let mut mesh = Mesh::load(model_path)?;
    mesh.limit_texture_size(512);
    let duration = mesh
        .animations
        .first()
        .map_or(0.0, |clip| clip.duration_seconds);
    let warm_frames = ((duration * f32::from(fps)).round() as usize).max(1);
    let area = Rect::new(0, 0, width, height);
    let config = Mesh3dConfig::default()
        .color_mode(ColorMode::Auto)
        .texture_filter(TextureFilter::Nearest)
        .backface_culling(true)
        .show_hints(false);
    let direct = PreparedMesh::new(&mesh);
    let cached = PreparedMesh::new(&mesh)
        .with_frame_cache(FrameCacheConfig::memory(fps).max_bytes(max_bytes));
    let mut direct_buffer = Buffer::empty(area);
    let mut cached_buffer = Buffer::empty(area);
    let state = Mesh3dState::default();

    // Prime reusable renderer scratch outside all measured sections.
    render_prepared_mesh(&direct, area, &mut direct_buffer, &state, &config);
    direct_buffer.reset();

    println!("model: {model_path}");
    println!(
        "viewport: {width}x{height}, animation: {duration:.6}s, cache rate: {fps} FPS, loop frames: {warm_frames}"
    );

    if mode == "direct" {
        let result = run(
            &direct,
            &mut direct_buffer,
            &config,
            fps,
            replay_frames,
            warm_frames,
            false,
        );
        print_result("direct", result);
        print_process_memory();
        return Ok(());
    }

    if mode == "cache" {
        let _ = run(&cached, &mut cached_buffer, &config, fps, 1, 0, false);
        let rss_after_first_frame = resident_bytes();
        let warm_result = run(
            &cached,
            &mut cached_buffer,
            &config,
            fps,
            warm_frames,
            warm_frames,
            false,
        );
        let replay_result = run(
            &cached,
            &mut cached_buffer,
            &config,
            fps,
            replay_frames,
            warm_frames,
            false,
        );
        let stats = cached.frame_cache_stats().expect("frame cache configured");
        print_result("cache warm-up", warm_result);
        print_result("cache replay", replay_result);
        print_cache_stats(stats);
        print_process_memory();
        println!(
            "RSS growth after first rendered frame: {:.2} MiB",
            resident_bytes().saturating_sub(rss_after_first_frame) as f64 / (1024.0 * 1024.0),
        );
        return Ok(());
    }

    let direct_result = run(
        &direct,
        &mut direct_buffer,
        &config,
        fps,
        replay_frames,
        warm_frames,
        true,
    );
    let warm_result = run(
        &cached,
        &mut cached_buffer,
        &config,
        fps,
        warm_frames,
        warm_frames,
        true,
    );
    let replay_result = run(
        &cached,
        &mut cached_buffer,
        &config,
        fps,
        replay_frames,
        warm_frames,
        true,
    );
    let stats = cached.frame_cache_stats().expect("frame cache configured");

    print_result("direct", direct_result);
    print_result("cache warm-up", warm_result);
    print_result("cache replay", replay_result);
    println!(
        "speedup: {:.2}x",
        direct_result.per_frame.as_secs_f64() / replay_result.per_frame.as_secs_f64(),
    );
    print_cache_stats(stats);
    print_process_memory();
    println!(
        "checksums (first loop): direct {:016x}, warm {:016x}, replay {:016x}",
        direct_result.loop_checksum, warm_result.loop_checksum, replay_result.loop_checksum
    );
    assert_eq!(direct_result.loop_checksum, warm_result.loop_checksum);
    assert_eq!(warm_result.loop_checksum, replay_result.loop_checksum);
    Ok(())
}

#[derive(Clone, Copy)]
struct RunResult {
    elapsed: Duration,
    per_frame: Duration,
    loop_checksum: u64,
    frames: usize,
    allocations: u64,
    allocated_bytes: u64,
}

fn run(
    prepared: &PreparedMesh<'_>,
    buf: &mut Buffer,
    config: &Mesh3dConfig,
    fps: u16,
    frames: usize,
    checksum_frames: usize,
    verify: bool,
) -> RunResult {
    let base_state = Mesh3dState::default();
    let mut elapsed = Duration::ZERO;
    let mut loop_checksum = 0_u64;
    let mut allocations = 0_u64;
    let mut allocated_bytes = 0_u64;
    for frame in 0..frames {
        buf.reset();
        let mut state = base_state;
        state.animation_time_seconds = frame as f32 / f32::from(fps);
        ALLOCATIONS.store(0, Ordering::Relaxed);
        ALLOCATED_BYTES.store(0, Ordering::Relaxed);
        COUNT_ALLOCATIONS.store(true, Ordering::Relaxed);
        let frame_started = Instant::now();
        render_prepared_mesh(prepared, buf.area, buf, &state, config);
        let frame_elapsed = frame_started.elapsed();
        COUNT_ALLOCATIONS.store(false, Ordering::Relaxed);
        elapsed += frame_elapsed;
        allocations += ALLOCATIONS.load(Ordering::Relaxed);
        allocated_bytes += ALLOCATED_BYTES.load(Ordering::Relaxed);
        if verify && frame < checksum_frames {
            loop_checksum ^= frame_checksum(black_box(buf)).rotate_left((frame % 64) as u32);
        }
    }
    RunResult {
        elapsed,
        per_frame: elapsed / frames as u32,
        loop_checksum,
        frames,
        allocations,
        allocated_bytes,
    }
}

fn print_result(label: &str, result: RunResult) {
    println!(
        "{label}: {} frames in {:.3}s, {:.3} ms/frame, {:.2} allocs/frame, {:.1} KiB/frame",
        result.frames,
        result.elapsed.as_secs_f64(),
        result.per_frame.as_secs_f64() * 1_000.0,
        result.allocations as f64 / result.frames as f64,
        result.allocated_bytes as f64 / result.frames as f64 / 1024.0,
    );
}

fn print_cache_stats(stats: ratatui_3dmesh::FrameCacheStats) {
    println!(
        "cache: {}/{} frames, {:.2} MiB logical, hits {}, misses {}, invalidations {}, limited {}",
        stats.cached_frames,
        stats.total_frames,
        stats.bytes as f64 / (1024.0 * 1024.0),
        stats.hits,
        stats.misses,
        stats.invalidations,
        stats.memory_limit_reached,
    );
}

fn print_process_memory() {
    println!(
        "RSS: {:.2} MiB; Cell: {} bytes",
        resident_bytes() as f64 / (1024.0 * 1024.0),
        size_of::<ratatui::buffer::Cell>(),
    );
}

fn frame_checksum(buf: &Buffer) -> u64 {
    let mut hasher = DefaultHasher::new();
    buf.content().hash(&mut hasher);
    hasher.finish()
}

fn parse_arg<T: std::str::FromStr>(args: &[String], index: usize, default: T) -> T {
    args.get(index)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[cfg(target_os = "linux")]
fn resident_bytes() -> usize {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    let resident_pages = statm
        .split_whitespace()
        .nth(1)
        .and_then(|pages| pages.parse::<usize>().ok())
        .unwrap_or(0);
    resident_pages * 4096
}

#[cfg(not(target_os = "linux"))]
fn resident_bytes() -> usize {
    0
}
