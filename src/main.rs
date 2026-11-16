//! EIS-NToy - Ventoy-style multiboot USB preparation tool (Linux).
//!
//! Prepares a USB drive/disk image with a boot partition and a data partition
//! that holds ISO/WIM/IMG files plus a generated boot menu configuration.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    println!("EIS-NToy v{VERSION} - Ventoy-style multiboot USB");
    println!();
    println!("Usage:");
    println!("  eis-ntoy init <dir>                 Create multiboot staging directory");
    println!(
        "  eis-ntoy scan <dir>                 Scan iso/ and (re)generate ventoy.json + grub.cfg"
    );
    println!("  eis-ntoy list <dir>                 List ISOs in staging dir");
    println!("  eis-ntoy build <dir> <out.img> <size_mb>   Build a raw disk image");
    println!("  eis-ntoy help");
}

fn cmd_init(dir: &str) {
    let root = Path::new(dir);
    for sub in ["iso", "boot", "config"] {
        let p = root.join(sub);
        if let Err(e) = fs::create_dir_all(&p) {
            eprintln!("create {p:?}: {e}");
            exit(1);
        }
    }
    let vj = root.join("ventoy.json");
    if !vj.exists() {
        let template = r#"{
  "control": [
    { "VTOY_DEFAULT_IMAGE": "" },
    { "VTOY_MENU_TIMEOUT": "10" }
  ],
  "theme": {},
  "menu_alias": [],
  "image_list": []
}
"#;
        let _ = fs::write(&vj, template);
    }
    let grub = root.join("config").join("grub.cfg");
    if !grub.exists() {
        let _ = fs::write(&grub, "# EIS-NToy generated grub.cfg\nset timeout=10\n");
    }
    println!("Initialized multiboot staging at {dir}");
    println!("Put ISO/WIM/IMG files into {dir}/iso/");
}

fn collect_isos(dir: &Path) -> Vec<PathBuf> {
    let iso_dir = dir.join("iso");
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&iso_dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() {
                let ext = p
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if matches!(ext.as_str(), "iso" | "wim" | "img" | "vhd" | "vhdX" | "efi") {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out
}

fn cmd_scan(dir: &str) {
    let root = Path::new(dir);
    let isos = collect_isos(root);
    // ventoy.json
    let mut entries = String::new();
    for iso in &isos {
        let name = iso.file_name().unwrap().to_string_lossy();
        entries.push_str(&format!(
            "  {{ \"image\": \"/iso/{name}\", \"alias\": \"{name}\" }},\n"
        ));
    }
    let json = format!(
        r#"{{
  "control": [
    {{ "VTOY_DEFAULT_IMAGE": "" }},
    {{ "VTOY_MENU_TIMEOUT": "10" }}
  ],
  "image_list": [
{}
  ]
}}
"#,
        entries.trim_end_matches(",\n")
    );
    let _ = fs::write(root.join("ventoy.json"), json);

    // grub.cfg
    let mut grub = String::from("# EIS-NToy generated grub.cfg\nset timeout=10\nmenuentry \"EIS-NToy\" { echo \"select image\"; }\n");
    for iso in &isos {
        let name = iso.file_name().unwrap().to_string_lossy();
        grub.push_str(&format!(
            "menuentry \"{name}\" {{\n  loopback loop /iso/{name}\n  # chainload handled by bootloader\n}}\n"
        ));
    }
    let _ = fs::write(root.join("config").join("grub.cfg"), grub);
    println!("Scanned {} image(s).", isos.len());
    for iso in &isos {
        println!("  {}", iso.display());
    }
}

fn cmd_list(dir: &str) {
    let isos = collect_isos(Path::new(dir));
    println!("{} image(s):", isos.len());
    for iso in &isos {
        let meta = fs::metadata(iso).ok();
        let sz = meta.map(|m| m.len()).unwrap_or(0);
        println!("  {} ({} MiB)", iso.display(), sz / (1024 * 1024));
    }
}

fn run(cmd: &str, args: &[&str]) -> bool {
    match Command::new(cmd).args(args).status() {
        Ok(s) => s.success(),
        Err(e) => {
            eprintln!("{cmd} failed: {e}");
            false
        }
    }
}

fn cmd_build(dir: &str, out: &str, size_mb: u64) {
    let root = Path::new(dir);
    let isos = collect_isos(root);
    // scan first
    cmd_scan(dir);

    let size_bytes = size_mb * 1024 * 1024;
    // create sparse image
    let f = match fs::File::create(out) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("create {out}: {e}");
            exit(1);
        }
    };
    f.set_len(size_bytes).unwrap();

    // partition with sfdisk (MBR): boot 32MB, rest data
    let sfdisk_input = "label: dos\n,32M,0c,*\n,,07\n";
    let mut child = Command::new("sfdisk")
        .arg(out)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .expect("sfdisk");
    use std::io::Write as _;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(sfdisk_input.as_bytes())
        .unwrap();
    let st = child.wait().unwrap();
    if !st.success() {
        eprintln!("sfdisk failed");
        exit(1);
    }
    // losetup
    let loopdev = Command::new("sudo")
        .args(["losetup", "--find", "--show", "--partscan", out])
        .output()
        .expect("losetup");
    if !loopdev.status.success() {
        eprintln!(
            "losetup failed: {}",
            String::from_utf8_lossy(&loopdev.stderr)
        );
        exit(1);
    }
    let dev = String::from_utf8_lossy(&loopdev.stdout).trim().to_string();
    println!("loop device: {dev}");

    // format partitions
    let p1 = format!("{dev}p1");
    let p2 = format!("{dev}p2");
    run("sudo", &["mkfs.vfat", "-F", "32", "-n", "EISBOOT", &p1]);
    run("sudo", &["mkfs.exfat", "-L", "EISDATA", &p2]);

    // mount p2 and copy files
    let mnt = "/tmp/eis-ntoy-mnt";
    let _ = fs::create_dir_all(mnt);
    run("sudo", &["mount", &p2, mnt]);
    // copy iso dir contents
    let iso_src = root.join("iso");
    if iso_src.exists() {
        run(
            "sudo",
            &["cp", "-r", &format!("{}/.", iso_src.display()), mnt],
        );
    }
    // copy ventoy.json and grub.cfg
    let _ = run(
        "sudo",
        &[
            "cp",
            &root.join("ventoy.json").to_string_lossy(),
            &format!("{}/ventoy.json", mnt),
        ],
    );
    let cfg_dir = format!("{}/config", mnt);
    let _ = run("sudo", &["mkdir", "-p", &cfg_dir]);
    let _ = run(
        "sudo",
        &[
            "cp",
            &root.join("config").join("grub.cfg").to_string_lossy(),
            &format!("{}/grub.cfg", cfg_dir),
        ],
    );
    run("sudo", &["umount", mnt]);

    // detach
    let _ = Command::new("sudo").args(["losetup", "-d", &dev]).status();
    println!(
        "Built {out} ({} MiB) with {} image(s).",
        size_mb,
        isos.len()
    );
    println!(
        "Note: bootloader installation (GRUB) requires grub-install; install manually if needed."
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_help();
        return;
    }
    match args[1].as_str() {
        "init" => {
            if args.len() < 3 {
                eprintln!("missing dir");
                exit(1);
            }
            cmd_init(&args[2]);
        }
        "scan" => {
            if args.len() < 3 {
                eprintln!("missing dir");
                exit(1);
            }
            cmd_scan(&args[2]);
        }
        "list" => {
            if args.len() < 3 {
                eprintln!("missing dir");
                exit(1);
            }
            cmd_list(&args[2]);
        }
        "build" => {
            if args.len() < 5 {
                eprintln!("usage: build <dir> <out.img> <size_mb>");
                exit(1);
            }
            let sz: u64 = args[4].parse().unwrap_or(1024);
            cmd_build(&args[2], &args[3], sz);
        }
        "help" | "--help" | "-h" => print_help(),
        _ => {
            eprintln!("unknown command: {}", args[1]);
            print_help();
            exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn init_creates_structure() {
        let dir = std::env::temp_dir().join("eis_ntoy_init");
        let _ = fs::remove_dir_all(&dir);
        cmd_init(dir.to_str().unwrap());
        assert!(dir.join("iso").is_dir());
        assert!(dir.join("boot").is_dir());
        assert!(dir.join("config").is_dir());
        assert!(dir.join("ventoy.json").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_generates_config() {
        let dir = std::env::temp_dir().join("eis_ntoy_scan");
        let _ = fs::remove_dir_all(&dir);
        cmd_init(dir.to_str().unwrap());
        // create a fake iso
        fs::write(dir.join("iso").join("test.iso"), b"fake").unwrap();
        cmd_scan(dir.to_str().unwrap());
        let vj = fs::read_to_string(dir.join("ventoy.json")).unwrap();
        assert!(vj.contains("test.iso"));
        let grub = fs::read_to_string(dir.join("config").join("grub.cfg")).unwrap();
        assert!(grub.contains("test.iso"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_isos_filters() {
        let dir = std::env::temp_dir().join("eis_ntoy_col");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("iso")).unwrap();
        fs::write(dir.join("iso").join("a.iso"), b"x").unwrap();
        fs::write(dir.join("iso").join("b.txt"), b"x").unwrap();
        let isos = collect_isos(&dir);
        assert_eq!(isos.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
