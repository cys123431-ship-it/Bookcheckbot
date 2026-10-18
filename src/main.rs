//! Windows 11용 전자책 화면 캡처 CLI.
//! 일반적인 화면 캡처와 키/마우스 입력만 사용하며 DRM/캡처 방지를 우회하지 않습니다.

use std::{
    fs,
    path::PathBuf,
    thread::sleep,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use enigo::{Enigo, Key, KeyboardControllable, MouseButton, MouseControllable};
use screenshots::Screen;

#[derive(Debug, Parser)]
#[command(
    name = "bookcheckbot",
    version,
    about = "허용된 전자책 뷰어 화면을 페이지별 PNG로 저장합니다 (개인 열람용)"
)]
struct Args {
    /// 마지막 페이지 번호(필수, 1 이상)
    #[arg(long)]
    pages: u32,

    /// 시작 페이지 번호. 뷰어도 같은 페이지로 직접 이동해야 합니다.
    #[arg(long, default_value_t = 1)]
    start: u32,

    /// PNG 저장 폴더
    #[arg(long, default_value = "shots")]
    out: PathBuf,

    /// 캡처 영역: Windows 가상 화면 기준 왼쪽 위 X 좌표
    #[arg(long, allow_hyphen_values = true)]
    x: Option<i32>,

    /// 캡처 영역: Windows 가상 화면 기준 왼쪽 위 Y 좌표
    #[arg(long, allow_hyphen_values = true)]
    y: Option<i32>,

    /// 캡처 영역 너비
    #[arg(long)]
    w: Option<u32>,

    /// 캡처 영역 높이
    #[arg(long)]
    h: Option<u32>,

    /// 캡처할 모니터 인덱스(0부터)
    #[arg(long, default_value_t = 0)]
    screen: usize,

    /// 페이지 넘김 방식: key 또는 click
    #[arg(long, value_enum, default_value_t = TurnMode::Key)]
    turn: TurnMode,

    /// 키보드 넘김 키: right, pagedown, space
    #[arg(long, default_value = "right")]
    key: String,

    /// 마우스로 넘길 때 클릭할 가상 화면 X 좌표
    #[arg(long, allow_hyphen_values = true)]
    click_x: Option<i32>,

    /// 마우스로 넘길 때 클릭할 가상 화면 Y 좌표
    #[arg(long, allow_hyphen_values = true)]
    click_y: Option<i32>,

    /// 캡처 전 카운트다운(초)
    #[arg(long, default_value_t = 5)]
    start_delay: u64,

    /// 캡처 직전 화면 렌더링 대기(ms)
    #[arg(long, default_value_t = 700)]
    render_wait: u64,

    /// 페이지 넘김 직후 대기(ms)
    #[arg(long, default_value_t = 800)]
    turn_wait: u64,
}

#[derive(Debug, Copy, Clone, ValueEnum, PartialEq, Eq)]
enum TurnMode {
    Key,
    Click,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

#[derive(Debug, Copy, Clone)]
struct MonitorBounds {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

#[derive(Debug, Copy, Clone)]
enum TurnAction {
    Key(Key),
    Click { x: i32, y: i32 },
}

struct Config {
    region: Option<Rect>,
    action: TurnAction,
}

// Windows 11에서 물리 픽셀 좌표를 일관되게 다루기 위해 DPI 인식을 우선 요청합니다.
// 실패해도 OS 보호 정책을 우회하지 않고, 100% 배율 사용을 안내합니다.
#[cfg(target_os = "windows")]
#[link(name = "user32")]
extern "system" {
    fn SetProcessDpiAwarenessContext(value: isize) -> i32;
}

#[cfg(target_os = "windows")]
fn enable_dpi_awareness() {
    // -4 = DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
    if unsafe { SetProcessDpiAwarenessContext(-4) } == 0 {
        eprintln!("안내: DPI 인식 설정이 적용되지 않았습니다. 배율 100%에서 좌표를 확인하세요.");
    }
}

#[cfg(not(target_os = "windows"))]
fn enable_dpi_awareness() {}

fn parse_key(name: &str) -> Result<Key> {
    match name.to_ascii_lowercase().as_str() {
        "right" | "rightarrow" | "right_arrow" => Ok(Key::RightArrow),
        "pagedown" | "page_down" | "next" => Ok(Key::PageDown),
        "space" | "spacebar" => Ok(Key::Space),
        other => bail!("지원하지 않는 --key 값: {other}. 사용 가능: right, pagedown, space"),
    }
}

fn validate_args(args: &Args) -> Result<Config> {
    if args.start == 0 || args.pages == 0 {
        bail!("--pages와 --start는 1 이상의 정수여야 합니다.");
    }
    if args.pages < args.start {
        bail!(
            "--pages({})는 --start({})보다 작을 수 없습니다.",
            args.pages,
            args.start
        );
    }

    let region = match (args.x, args.y, args.w, args.h) {
        (None, None, None, None) => None,
        (Some(x), Some(y), Some(w), Some(h)) => {
            if w == 0 || h == 0 {
                bail!("영역 크기 --w와 --h는 0보다 커야 합니다.");
            }
            Some(Rect { x, y, w, h })
        }
        _ => bail!("영역 캡처에는 --x --y --w --h 네 옵션이 모두 필요합니다."),
    };

    let action = match args.turn {
        TurnMode::Key => TurnAction::Key(parse_key(&args.key)?),
        TurnMode::Click => TurnAction::Click {
            x: args
                .click_x
                .context("--turn click 사용 시 --click-x가 필요합니다.")?,
            y: args
                .click_y
                .context("--turn click 사용 시 --click-y가 필요합니다.")?,
        },
    };

    Ok(Config { region, action })
}

/// 사용자 입력은 Windows 전체 화면의 절대 좌표.
/// screenshots::Screen::capture_area는 선택한 모니터 내 상대 좌표를 받습니다.
/// 라이브러리는 바깥쪽 영역을 조용히 잘라낼 수 있으므로 명시적으로 거부합니다.
fn to_monitor_relative(region: Rect, screen: MonitorBounds) -> Result<Rect> {
    let left = i64::from(region.x);
    let top = i64::from(region.y);
    let right = left + i64::from(region.w);
    let bottom = top + i64::from(region.h);
    let sx = i64::from(screen.x);
    let sy = i64::from(screen.y);
    let sx2 = sx + i64::from(screen.w);
    let sy2 = sy + i64::from(screen.h);

    if left < sx || top < sy || right > sx2 || bottom > sy2 {
        bail!(
            "지정한 캡처 영역 ({}, {}, {}, {})이 선택한 모니터의 범위 ({}, {}, {}, {}) 밖입니다.",
            region.x,
            region.y,
            region.w,
            region.h,
            screen.x,
            screen.y,
            screen.w,
            screen.h
        );
    }

    Ok(Rect {
        x: i32::try_from(left - sx).context("영역 X 좌표 변환 실패")?,
        y: i32::try_from(top - sy).context("영역 Y 좌표 변환 실패")?,
        w: region.w,
        h: region.h,
    })
}

fn filename(page: u32) -> String {
    format!("page_{page:04}.png")
}

/// 렌더링/입력 부분을 주입 가능하게 분리해 GUI 없는 테스트에서도 페이지 흐름을 검증합니다.
fn capture_sequence<C, T>(start: u32, last: u32, mut capture: C, mut turn: T) -> Result<()>
where
    C: FnMut(u32) -> Result<()>,
    T: FnMut() -> Result<()>,
{
    for page in start..=last {
        capture(page)?;
        if page < last {
            turn()?;
        }
    }
    Ok(())
}

fn appears_blank(image: &screenshots::image::RgbaImage) -> bool {
    // 표본이 거의 전부 흰색/검정색이면 차단 또는 진짜 빈 페이지일 수 있습니다.
    // 잘못된 확정을 피하기 위해 '가능성'으로만 안내합니다.
    let mut samples = 0usize;
    let mut light = 0usize;
    let mut dark = 0usize;
    for p in image.pixels().step_by(503) {
        samples += 1;
        let rgb = &p.0[..3];
        if rgb.iter().all(|&c| c >= 250) {
            light += 1;
        }
        if rgb.iter().all(|&c| c <= 5) {
            dark += 1;
        }
    }
    samples > 0 && (light * 1000 >= samples * 995 || dark * 1000 >= samples * 995)
}

fn main() -> Result<()> {
    let args = Args::parse();
    let config = validate_args(&args)?;
    enable_dpi_awareness();

    let screens = Screen::all().context("모니터 목록을 가져오지 못했습니다.")?;
    if args.screen >= screens.len() {
        bail!(
            "모니터 인덱스 {}를 찾을 수 없습니다. 사용 가능: 0..{}",
            args.screen,
            screens.len().saturating_sub(1)
        );
    }

    let screen = screens[args.screen];
    let info = screen.display_info;
    let bounds = MonitorBounds {
        x: info.x,
        y: info.y,
        w: info.width,
        h: info.height,
    };
    let relative_region = config
        .region
        .map(|rect| to_monitor_relative(rect, bounds))
        .transpose()?;

    fs::create_dir_all(&args.out)
        .with_context(|| format!("저장 폴더 생성 실패: {}", args.out.display()))?;

    let mut enigo = Enigo::new();
    println!("=== Bookcheckbot: 전자책 화면 캡처 ===");
    println!("저장 폴더: {}", args.out.display());
    println!("페이지 범위: {} ~ {}", args.start, args.pages);
    println!(
        "모니터 {}: 화면 원점=({}, {}), 크기={}x{}",
        args.screen, info.x, info.y, info.width, info.height
    );
    if let Some(rect) = config.region {
        println!("선택 영역(절대 좌표): x={}, y={}, w={}, h={}", rect.x, rect.y, rect.w, rect.h);
    } else {
        println!("캡처 영역: 선택한 모니터 전체");
    }
    println!("페이지 넘김 방식: {:?}", args.turn);
    println!("주의: 이 도구는 DRM이나 캡처 제한을 우회하지 않습니다.");
    println!("주의: 뷰어를 시작 페이지에 직접 맞추고 창 크기를 고정하세요.");

    if args.start_delay > 0 {
        println!(
            "{}초 뒤 시작합니다. 전자책 뷰어에 포커스를 맞춰 주세요.",
            args.start_delay
        );
        for remaining in (1..=args.start_delay).rev() {
            println!("{remaining}...");
            sleep(Duration::from_secs(1));
        }
    }
    println!("캡처 중에는 키보드와 마우스를 건드리지 마세요. 중단: Ctrl+C");

    capture_sequence(
        args.start,
        args.pages,
        |page| {
            sleep(Duration::from_millis(args.render_wait));
            let path = args.out.join(filename(page));
            let image = match relative_region {
                Some(rect) => screen.capture_area(rect.x, rect.y, rect.w, rect.h),
                None => screen.capture(),
            }
            .with_context(|| format!("{page}페이지 캡처 실패. 캡처가 차단되었다면 우회하지 말고 중단하세요."))?;

            image
                .save(&path)
                .with_context(|| format!("{page}페이지 PNG 저장 실패: {}", path.display()))?;

            println!("[{page}/{}] 저장: {}", args.pages, path.display());

            if appears_blank(&image) {
                bail!(
                    "{page}페이지가 거의 완전히 흰색 또는 검정색입니다. 실제 빈 페이지일 수도 있고 캡처가 차단되었을 수도 있습니다. 저장된 이미지를 확인하세요. 화면이 미완성이라면 --render-wait/--turn-wait를 조절하고, DRM/캡처 제한이면 우회하지 말고 중단하세요."
                );
            }
            Ok(())
        },
        || {
            match config.action {
                TurnAction::Key(key) => enigo.key_click(key),
                TurnAction::Click { x, y } => {
                    enigo.mouse_move_to(x, y);
                    sleep(Duration::from_millis(50));
                    enigo.mouse_click(MouseButton::Left);
                }
            }
            sleep(Duration::from_millis(args.turn_wait));
            Ok(())
        },
    )?;

    println!("완료! 출력 폴더: {}", args.out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Args {
        Args::try_parse_from(parts).expect("CLI 파싱 성공")
    }

    #[test]
    fn default_options_are_applied() {
        let a = args(&["bookcheckbot", "--pages", "3"]);
        assert_eq!(a.start, 1);
        assert_eq!(a.screen, 0);
        assert_eq!(a.turn, TurnMode::Key);
        assert_eq!(a.out, PathBuf::from("shots"));
        assert_eq!(a.render_wait, 700);
        assert_eq!(a.turn_wait, 800);
    }

    #[test]
    fn invalid_page_range_is_rejected() {
        assert!(validate_args(&args(&["bookcheckbot", "--pages", "1", "--start", "2"])).is_err());
        assert!(validate_args(&args(&["bookcheckbot", "--pages", "0"])).is_err());
        assert!(validate_args(&args(&["bookcheckbot", "--pages", "2", "--start", "0"])).is_err());
    }

    #[test]
    fn partial_and_empty_regions_are_rejected() {
        assert!(validate_args(&args(&["bookcheckbot", "--pages", "3", "--x", "10"])).is_err());
        assert!(validate_args(&args(&[
            "bookcheckbot", "--pages", "3", "--x", "10", "--y", "20",
            "--w", "0", "--h", "200",
        ])).is_err());
    }

    #[test]
    fn click_mode_needs_both_coordinates() {
        assert!(validate_args(&args(&["bookcheckbot", "--pages", "3", "--turn", "click"])).is_err());
        assert!(validate_args(&args(&[
            "bookcheckbot", "--pages", "3", "--turn", "click", "--click-x", "150",
        ])).is_err());
        assert!(validate_args(&args(&[
            "bookcheckbot", "--pages", "3", "--turn", "click",
            "--click-x", "150", "--click-y", "250",
        ])).is_ok());
    }

    #[test]
    fn allowed_keys_and_unsupported_key() {
        for key in ["right", "pagedown", "space"] {
            assert!(parse_key(key).is_ok());
        }
        assert!(parse_key("enter").is_err());
    }

    #[test]
    fn monitor_coordinates_support_negative_offsets() {
        let relative = to_monitor_relative(
            Rect { x: -1800, y: 100, w: 500, h: 600 },
            MonitorBounds { x: -1920, y: 0, w: 1920, h: 1080 },
        )
        .unwrap();
        assert_eq!(relative, Rect { x: 120, y: 100, w: 500, h: 600 });
    }

    #[test]
    fn region_outside_selected_monitor_fails() {
        assert!(to_monitor_relative(
            Rect { x: 1700, y: 0, w: 300, h: 100 },
            MonitorBounds { x: 0, y: 0, w: 1920, h: 1080 },
        )
        .is_err());
    }

    #[test]
    fn three_pages_create_three_names_and_only_two_turns() {
        let mut names = Vec::new();
        let mut turns = 0u32;
        capture_sequence(
            1, 3,
            |page| { names.push(filename(page)); Ok(()) },
            || { turns += 1; Ok(()) },
        )
        .unwrap();
        assert_eq!(names, ["page_0001.png", "page_0002.png", "page_0003.png"]);
        assert_eq!(turns, 2);
    }

    #[test]
    fn two_pages_and_resuming_work() {
        let mut pages = Vec::new();
        capture_sequence(1, 2, |page| { pages.push(page); Ok(()) }, || Ok(())).unwrap();
        assert_eq!(pages, [1, 2]);
        pages.clear();
        capture_sequence(151, 153, |page| { pages.push(page); Ok(()) }, || Ok(())).unwrap();
        assert_eq!(pages, [151, 152, 153]);
    }

    #[test]
    fn blank_screen_detection() {
        let white = screenshots::image::RgbaImage::from_pixel(
            20, 20, screenshots::image::Rgba([255, 255, 255, 255])
        );
        assert!(appears_blank(&white));
        let grey = screenshots::image::RgbaImage::from_pixel(
            20, 20, screenshots::image::Rgba([120, 120, 120, 255])
        );
        assert!(!appears_blank(&grey));
    }
}
