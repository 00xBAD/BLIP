fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("icon.ico"); // path relativo al root del progetto
        res.compile().expect("Failed to compile Windows resources");
    }
}
