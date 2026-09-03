//! serverbrowser — a terminal browser on the Servo engine.

use std::io::Write;
use std::time::Duration;

use serverbrowser::config::{Config, OutputMode};
use serverbrowser::mindmap::Mindmap;
use serverbrowser::output::{emit, Frame};
use serverbrowser::render::Engine;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = Config::load(&config_path()).unwrap_or_else(|e| {
        eprintln!("config error: {e}; using defaults");
        Config::default()
    });

    match args.first().map(|s| s.as_str()) {
        Some("bookmark-add") | Some("ba") => cmd_bookmark_add(&cfg, &args[1..]),
        Some("bookmarks") | Some("bm") => cmd_bookmarks(&cfg),
        Some("render") | Some("r") => cmd_render(&cfg, &args[1..]),
        Some("open") | Some("o") | None => cmd_open(&cfg, &args[..]),
        Some("help") | Some("-h") | Some("--help") => print_help(),
        Some(other) => {
            eprintln!("unknown subcommand: {other}");
            print_help();
        }
    }
}

fn config_path() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".config/serverbrowser/config.toml"))
        .unwrap_or_else(|_| std::path::PathBuf::from("config.toml"))
}

fn print_help() {
    println!(
        "serverbrowser — a terminal browser on the Servo engine\n\
         \n\
         USAGE:\n\
           serverbrowser [open|o] [URL]       open a URL (interactive; falls back to render)\n\
           serverbrowser render|r [URL]       render a URL once and print it\n\
           serverbrowser bookmark-add|ba <URL> [parent-title]\n\
           serverbrowser bookmarks|bm         list bookmarks (mindmap nodes)\n\
         \n\
         ENV:\n\
           SERVERBROWSER_OUTPUT  kitty|sixel|blocks|text  (override output mode)\n"
    );
}

/// Default: open a URL interactively (or render once if piped).
fn cmd_open(cfg: &Config, args: &[String]) {
    let url = args
        .get(1)
        .cloned()
        .or_else(|| args.get(0).cloned())
        .filter(|s| !s.starts_with("open"))
        .unwrap_or_else(|| cfg.start_page.clone());

    // If stdout is not a TTY, render once and exit (scriptable "shot" mode).
    if !atty_stdio() {
        render_once(cfg, &url);
        return;
    }

    // Interactive mode: a minimal loop. We make one full render and display it,
    // then accept `:` commands via stdin.
    interactive(cfg, &url);
}

fn cmd_render(cfg: &Config, args: &[String]) {
    let url = args.first().cloned().unwrap_or_else(|| cfg.start_page.clone());
    render_once(cfg, &url);
}

fn render_once(cfg: &Config, url: &str) {
    match Engine::new(cfg.viewport_width, cfg.viewport_height) {
        Ok(engine) => {
            if let Err(e) = engine.load(url) {
                eprintln!("load error: {e}");
                return;
            }
            if engine.wait_for_frame(Duration::from_secs(20)) {
                if let Some(pixels) = engine.snapshot() {
                    let frame = Frame::from_rgba(
                        cfg.viewport_width,
                        cfg.viewport_height,
                        pixels,
                    );
                    let mut stdout = std::io::stdout();
                    let _ = emit(&mut stdout, &frame, resolve_output(cfg));
                    let _ = stdout.flush();
                } else {
                    eprintln!("no frame captured");
                }
            } else {
                eprintln!("timed out waiting for render");
            }
        }
        Err(e) => eprintln!("failed to init engine: {e}"),
    }
}

fn interactive(cfg: &Config, url: &str) {
    let mut command = String::new();
    // Show a status line and prompt, then read a command.
    println!("serverbrowser — loading {url}");
    match Engine::new(cfg.viewport_width, cfg.viewport_height) {
        Ok(engine) => {
            if engine.load(url).is_ok() && engine.wait_for_frame(Duration::from_secs(20)) {
                if let Some(pixels) = engine.snapshot() {
                    let frame = Frame::from_rgba(cfg.viewport_width, cfg.viewport_height, pixels);
                    let mut stdout = std::io::stdout();
                    let _ = emit(&mut stdout, &frame, resolve_output(cfg));
                    let _ = stdout.flush();
                }
            }
            println!();
            println!("Press :, then a command (:open URL, :back, :reload, :quit).");
            loop {
                print!(": ");
                let _ = std::io::stdout().flush();
                command.clear();
                match std::io::stdin().read_line(&mut command) {
                    Ok(0) => break,
                    Ok(_) => {
                        let cmd = serverbrowser::nav::parse_command(&command);
                        use serverbrowser::nav::Command as C;
                        match cmd {
                            C::Quit => break,
                            C::Open { url, .. } => {
                                if engine.load(&url).is_ok()
                                    && engine.wait_for_frame(Duration::from_secs(20))
                                {
                                    if let Some(px) = engine.snapshot() {
                                        let f = Frame::from_rgba(
                                            cfg.viewport_width,
                                            cfg.viewport_height,
                                            px,
                                        );
                                        let mut o = std::io::stdout();
                                        let _ = emit(&mut o, &f, resolve_output(cfg));
                                    }
                                }
                            }
                            C::Back => engine.go_back(),
                            C::Forward => engine.go_forward(),
                            C::Reload => engine.reload(),
                            C::BookmarkAdd { url: u, parent } => {
                                let u = u.or_else(|| engine.url()).unwrap_or_default();
                                let title = engine.title().unwrap_or_else(|| u.clone());
                                let mut mm = Mindmap::open(&cfg.vault_dir);
                                match mm.add_bookmark(&title, &u, parent.as_deref()) {
                                    Ok(id) => println!("bookmarked -> {id}"),
                                    Err(e) => eprintln!("bookmark error: {e}"),
                                }
                            }
                            _ => eprintln!("command not handled in this build"),
                        }
                    }
                    Err(e) => {
                        eprintln!("read error: {e}");
                        break;
                    }
                }
            }
        }
        Err(e) => eprintln!("failed to init engine: {e}"),
    }
}

fn cmd_bookmark_add(cfg: &Config, args: &[String]) {
    if args.is_empty() {
        eprintln!("usage: serverbrowser bookmark-add <URL> [parent-title]");
        return;
    }
    let url = args[0].clone();
    let parent = args.get(1).map(|s| s.as_str());
    let title = title_from_url(&url);
    let mut mm = Mindmap::open(&cfg.vault_dir);
    match mm.add_bookmark(&title, &url, parent) {
        Ok(id) => println!("bookmarked \"{title}\" ({id}) -> {}", cfg.vault_dir.display()),
        Err(e) => eprintln!("error: {e}"),
    }
}

fn cmd_bookmarks(cfg: &Config) {
    let mm = Mindmap::open(&cfg.vault_dir);
    if mm.nodes.is_empty() {
        println!("no bookmarks yet in {}", cfg.vault_dir.display());
        return;
    }
    println!("Bookmarks (mindmap nodes) in {}:\n", cfg.vault_dir.display());
    for kid in &mm.nodes {
        let (id, n) = kid;
        println!(
            "  [{}] {}  {}{}",
            id,
            n.title,
            n.url.as_deref().unwrap_or(""),
            if n.links.is_empty() {
                String::new()
            } else {
                format!("  -> links: {}", n.links.join(", "))
            }
        );
    }
    let edges = mm.edges();
    if !edges.is_empty() {
        println!("\nEdges (for the future minimap):");
        for (a, b) in edges {
            println!("  {a} <-> {b}");
        }
    }
}

fn title_from_url(url: &str) -> String {
    url.trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

fn resolve_output(cfg: &Config) -> OutputMode {
    if let Ok(mode) = std::env::var("SERVERBROWSER_OUTPUT") {
        match mode.as_str() {
            "kitty" => return OutputMode::Kitty,
            "sixel" => return OutputMode::Sixel,
            "blocks" => return OutputMode::Blocks,
            "text" => return OutputMode::Text,
            _ => {}
        }
    }
    cfg.output_mode
}

fn atty_stdio() -> bool {
    // Best-effort: assume interactive unless stdout is a pipe.
    use std::os::unix::io::AsRawFd;
    let fd = std::io::stdout().as_raw_fd();
    // isatty(3) style check via libc-free approach is unavailable; use a
    // heuristic: if TERM is unset, likely not a terminal.
    std::env::var("TERM").map(|t| !t.is_empty()).unwrap_or(false)
}