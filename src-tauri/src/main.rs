// Always a plain window app on Windows: no terminal window, in test builds or release builds.
#![windows_subsystem = "windows"]

fn main() {
    modnest_lib::run()
}
