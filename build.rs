fn main() {
    println!("cargo:rerun-if-changed=assets/nanodns.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/nanodns.ico");
        resource.set("ProductName", "nanoDNS Desktop");
        resource.set("FileDescription", "nanoDNS Desktop DNS proxy");
        resource.set(
            "LegalCopyright",
            "nanoDNS (c) Drakmor; desktop port GPL-3.0",
        );
        if let Err(error) = resource.compile() {
            panic!("Unable to compile Windows application resources: {error}");
        }
    }
}
