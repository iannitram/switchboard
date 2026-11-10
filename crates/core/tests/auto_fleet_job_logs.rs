//! #298: the fleet launcher's stdout and stderr land in `<job id>.out` and `<job id>.err`
//! beside the job file, so a crash leaves its reason on disk.

use std::path::PathBuf;
use switchboard_core::autopilot::passes::job_log_files;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-298-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn job_log_files_creates_out_and_err_beside_the_job_file() {
    let dir = temp_dir("job-logs");
    let job_path = dir.join("job-42.json");
    std::fs::write(&job_path, "{}").unwrap();

    let (mut out, mut err) = job_log_files(&job_path).unwrap();
    std::io::Write::write_all(&mut out, b"stdout line\n").unwrap();
    std::io::Write::write_all(&mut err, b"stderr line\n").unwrap();
    drop((out, err));

    assert_eq!(
        std::fs::read_to_string(dir.join("job-42.out")).unwrap(),
        "stdout line\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("job-42.err")).unwrap(),
        "stderr line\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
