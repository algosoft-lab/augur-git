//! Windows 可执行文件图标（镜像 augur-pdf build.rs）
//!
//! assets/algogit.ico 由 assets/algogit.svg 经 ImageMagick 多尺寸渲染打包
//! （16/24/32/48/64/128/256），重生成命令见 AGENTS.md。

fn main() {
    println!("cargo:rerun-if-changed=assets/algogit.ico");

    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/algogit.ico");
        res.compile().expect("Failed to compile Windows resource");
    }
}
