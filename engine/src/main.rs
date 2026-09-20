//! MaaCoC engine command line.
//!
//! Doubles as the engine's smoke test: every capability reachable from the UI
//! is reachable here without a window, so it can be verified headlessly.

use maacoc_engine::{
    frames::FrameStore,
    pipeline::{Edit, PipelineDoc},
    trial::trial_node,
    DeviceTarget, Error, NodeEvent, Runner,
};
use maa_framework::toolkit::Toolkit;
use std::{
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

fn main() {
    if let Err(err) = run() {
        eprintln!("错误: {err}");
        std::process::exit(1);
    }
}

fn usage() {
    println!(
        "用法: maacoc-engine <命令> [参数]

  devices                        列出 ADB 设备
  snap [输出.png]                 截一帧，报告设备与匹配空间尺寸
  load [assets]                  加载资源并列出节点
  run [--minutes N] [--entry E]  跑自动战斗循环并打印节点时间轴
  reco <节点> <帧.png> [阈值]     在一张静态帧上试跑该节点的识别
  regress [语料目录]              对已录制的帧重跑识别，检查资产退化
  patch <节点> <字段> <JSON> [--write]
                                 定点改写 pipeline；默认只打印 diff，加 --write 才落盘"
    );
}

fn run() -> Result<(), Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    Toolkit::init_option("var", "{}")?;
    let assets_arg = args
        .iter()
        .position(|a| a == "--assets")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "assets".to_string());
    let assets = Path::new(&assets_arg);
    let preferred = args.iter().position(|a| a == "--device").and_then(|i| args.get(i + 1)).cloned();

    match args.first().map(String::as_str) {
        Some("devices") => devices(),
        Some("snap") => snap(assets, args.get(1).map(String::as_str), preferred.as_deref()),
        Some("load") => load(assets),
        Some("run") => battle(assets, &args, preferred.as_deref()),
        Some("reco") => reco(assets, &args),
        Some("regress") => regress(assets, args.get(1).cloned()),
        Some("patch") => patch(assets, &args),
        _ => {
            usage();
            Ok(())
        }
    }
}

fn devices() -> Result<(), Error> {
    let devices = Toolkit::find_adb_devices()?;
    if devices.is_empty() {
        println!("没有找到 ADB 设备");
        return Ok(());
    }
    for device in &devices {
        println!("{} @ {} (adb={})", device.name, device.address, device.adb_path.display());
    }
    Ok(())
}

fn snap(assets: &Path, output: Option<&str>, preferred: Option<&str>) -> Result<(), Error> {
    let runner = Runner::connect(assets, DeviceTarget::Adb, preferred)?;
    let png = runner.screencap_png()?;
    let target = output.unwrap_or("var/snap.png");
    if let Some(parent) = Path::new(target).parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(target, &png)?;
    println!("{} 已保存 {}（{} 字节，匹配空间 1280x720）", runner.label, target, png.len());
    Ok(())
}

fn load(assets: &Path) -> Result<(), Error> {
    let runner = Runner::connect(assets, DeviceTarget::Headless, None)?;
    println!("资源哈希: {}", runner.resource().hash()?);
    println!("节点数: {}", runner.resource().node_list()?.len());
    println!("Tasker 就绪: {}", runner.inited());
    Ok(())
}

fn print_event(event: &NodeEvent) {
    println!("[{:7.1}s] {}", event.at, event.summary());
}

fn battle(assets: &Path, args: &[String], preferred: Option<&str>) -> Result<(), Error> {
    let minutes: f64 = args
        .iter()
        .position(|a| a == "--minutes")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(5.0);
    let entry = args
        .iter()
        .position(|a| a == "--entry")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .unwrap_or("Main");

    let runner = Runner::connect(assets, DeviceTarget::Adb, preferred)?;
    println!("{} 资源就绪，投递任务 {entry}", runner.label);
    runner.start(entry)?;

    let deadline = Instant::now() + Duration::from_secs_f64(minutes * 60.0);
    let mut total = 0usize;
    let mut starts = 0usize;
    let mut last: Option<(String, String)> = None;
    let mut repeat = 0usize;

    while Instant::now() < deadline {
        for event in runner.poll() {
            total += 1;
            if event.kind == "action" && event.node == "AttackStart" {
                starts += 1;
            }
            let key = (event.node.clone(), event.kind.to_string());
            if last.as_ref() == Some(&key) {
                repeat += 1;
                continue;
            }
            if repeat > 0 {
                if let Some((previous, _)) = &last {
                    println!("  {previous} x{}", repeat + 1);
                }
            }
            last = Some(key);
            repeat = 0;
            print_event(&event);
        }
        if !runner.running() {
            println!("任务已结束");
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    runner.stop(Duration::from_secs(15));
    println!("完成: 开局 {starts} 次，事件 {total} 条，丢弃 {} 条", runner.dropped_events());
    Ok(())
}

fn open_offline_runner(assets: &Path, _frame: &Path) -> Result<Runner, Error> {
    Runner::connect(assets, DeviceTarget::Headless, None)
}

fn reco(assets: &Path, args: &[String]) -> Result<(), Error> {
    let (Some(node), Some(frame)) = (args.get(1), args.get(2)) else {
        return Err("用法: reco <节点> <帧.png> [阈值]".into());
    };
    let threshold: Option<f64> = args.get(3).and_then(|v| v.parse().ok());
    let frame_path = Path::new(frame);
    let png = std::fs::read(frame_path)?;
    let runner = open_offline_runner(assets, frame_path)?;
    let trial = trial_node(runner.resource(), runner.tasker(), runner.bus(), node, &png, threshold, None)?;
    println!("{}", trial.summary());
    Ok(())
}

fn regress(assets: &Path, dir: Option<String>) -> Result<(), Error> {
    let root = PathBuf::from(dir.unwrap_or_else(|| "tests/fixtures/frames".to_string()));
    let store = FrameStore::open(&root);
    let frames = store.frames()?;
    if frames.is_empty() {
        return Err(format!("{} 里没有语料帧", root.display()).into());
    }
    let runner = Runner::connect(assets, DeviceTarget::Headless, None)?;

    let mut failed = 0usize;
    for record in &frames {
        let png = store.load_png(record)?;
        let trial = trial_node(runner.resource(), runner.tasker(), runner.bus(), &record.node, &png, None, None)?;
        let ok = trial.hit && trial.error.is_none();
        if !ok {
            failed += 1;
        }
        println!("{} {} {}", if ok { "OK  " } else { "失败" }, record.file, trial.summary());
    }
    println!("共 {} 帧，失败 {failed}", frames.len());
    if failed > 0 {
        std::io::stdout().flush().ok();
        return Err("识别回归失败".into());
    }
    Ok(())
}

fn patch(assets: &Path, args: &[String]) -> Result<(), Error> {
    let (Some(node), Some(field), Some(value)) = (args.get(1), args.get(2), args.get(3)) else {
        return Err("用法: patch <节点> <字段> <JSON> [--write]".into());
    };
    let parsed: serde_json::Value = serde_json::from_str(value)
        .map_err(|e| format!("第三个参数必须是合法 JSON（数组要写成 [\"a\"]）: {e}"))?;
    let file = assets.join("pipeline/main.json");
    let mut doc = PipelineDoc::open(&file)?;
    let edits = [Edit::new(node.clone(), field.clone(), parsed)];
    print!("{}", doc.diff(&edits)?);
    if args.iter().any(|a| a == "--write") {
        doc.write(&edits)?;
        println!("已写入 {}", file.display());
    } else {
        println!("（未写入；加 --write 落盘）");
    }
    Ok(())
}
