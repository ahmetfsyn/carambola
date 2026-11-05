fn main() {
    slint_build::compile("ui/windows.slint").expect("compile main native UI");
    slint_build::compile("ui/overlay.slint").expect("compile transparent overlay prototype");
}
