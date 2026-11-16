use std::fs;
use std::path::Path;

// {cite}`habeas:util`
pub fn coverage_setup(root: &Path) {
    let (path, source) = habeas::execution_coverage::configure(root).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}
