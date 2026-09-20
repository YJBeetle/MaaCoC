//! MaaCoC engine probes.
//!
//! Three commands that together prove the risky seam works on this platform:
//! the vendored SDK loads, a device can be driven, and the existing
//! `assets/` bundle parses. UI comes after this is green.

use maa_framework::{common, controller::Controller, resource::Resource, toolkit::Toolkit};
use std::{error::Error, path::Path};

fn main() {
    if let Err(err) = run() {
        eprintln!("错误: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // MaaFramework's own option file drives logging/stdout level.
    Toolkit::init_option("var", "{}")?;

    match args.first().map(String::as_str) {
        Some("devices") => devices(),
        Some("snap") => snap(args.get(1).map(String::as_str)),
        Some("load") => load(args.get(1).map(String::as_str).unwrap_or("assets")),
        _ => {
            println!("用法: maacoc-engine <devices|snap [out.png]|load [assets目录]>");
            Ok(())
        }
    }
}

fn first_device() -> Result<maa_framework::toolkit::AdbDevice, Box<dyn Error>> {
    let devices = Toolkit::find_adb_devices()?;
    devices
        .into_iter()
        .next()
        .ok_or_else(|| "没有找到 ADB 设备".into())
}

fn devices() -> Result<(), Box<dyn Error>> {
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

fn snap(output: Option<&str>) -> Result<(), Box<dyn Error>> {
    let device = first_device()?;
    let config = serde_json::to_string(&device.config)?;
    let controller = Controller::new_adb(
        device.adb_path.to_str().ok_or("adb 路径无效")?,
        &device.address,
        &config,
        "",
    )?;

    controller.wait(controller.post_connection()?);
    if !controller.connected() {
        return Err(format!("连接 {} 失败", device.address).into());
    }

    // Resolution is only meaningful after the first frame has been captured.
    let cap_id = controller.post_screencap()?;
    let cap_status = controller.wait(cap_id);
    let (raw_w, raw_h) = controller.resolution()?;

    let image = controller.cached_image()?;
    let width = image.width();
    let height = image.height();
    println!("截图任务状态: {:?}", cap_status);

    // The whole project's coordinates live in MaaFramework's downscaled
    // screenshot space (short side 720), not the device's native pixels.
    println!("设备原始分辨率 {raw_w}x{raw_h}；匹配用截图 {width}x{height} 通道 {}", image.channels());

    let target = output.unwrap_or("var/snap.png");
    if let Some(png) = image.to_vec() {
        if let Some(parent) = Path::new(&target).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(&target, png)?;
        println!("已保存 {target}");
    } else {
        println!("截图为空（未取到编码数据）");
    }
    Ok(())
}

fn load(assets: &str) -> Result<(), Box<dyn Error>> {
    let resource = Resource::new()?;
    let status = resource.post_bundle(assets)?.wait();
    println!("bundle 加载状态: {:?}", status);
    if status != common::MaaStatus::SUCCEEDED || !resource.loaded() {
        return Err("资源加载失败".into());
    }
    println!("节点数: {}", resource.node_list()?.len());
    println!("资源哈希: {}", resource.hash()?);
    Ok(())
}
