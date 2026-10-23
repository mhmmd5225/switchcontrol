//! Embeds the icon and file description into the Windows executable.

fn main() {
    println!("cargo:rerun-if-changed=../assets/switchcontrol.rc");
    println!("cargo:rerun-if-changed=../assets/icon.ico");
    // Does nothing when not building for Windows. Without a resource
    // compiler the program still builds, just without an icon.
    if let Err(e) = embed_resource::compile("../assets/switchcontrol.rc", embed_resource::NONE)
        .manifest_optional()
    {
        panic!("embedding the Windows resources failed: {e}");
    }
}
