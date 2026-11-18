//! Embeds the app icon (assets/icon.ico, via assets/pane-pogo.rc) into the exe, so
//! Explorer, the taskbar and Alt+Tab show it. The tray loads the same resource.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/pane-pogo.manifest");
    embed_resource::compile("assets/pane-pogo.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("could not embed assets/pane-pogo.rc");
}
