extern crate embed_resource;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() != "windows" { return; }
    let _ = embed_resource::compile("buildsrc/augment-vip.rc", embed_resource::NONE);
}