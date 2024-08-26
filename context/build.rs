fn main() {
    #[cfg(feature = "boost")]
    build_boost();
}

#[cfg(feature = "boost")]
fn build_boost() {
    use std::{
        env, fs,
        io::{BufRead, BufReader},
        path::{Path, PathBuf},
    };

    let target = env::var("TARGET").unwrap();
    let is_win_gnu = target.ends_with("windows-gnu");
    let is_win_msvc = target.ends_with("windows-msvc");
    let is_win = is_win_gnu || is_win_msvc;

    let arch = match target.split('-').next().unwrap() {
        "arm" | "armv7" | "armv7s" => "arm",
        "arm64" | "aarch64" => "arm64",
        "x86" | "i386" | "i486" | "i586" | "i686" => "i386",
        "mips" | "mipsel" => "mips32",
        "x86_64" => "x86_64",
        "mips64" | "mips64el" => "mips64",
        "loongarch64" => "loongarch64",
        "riscv64gc" => "riscv64",
        "s390x" => "s390x",
        _ => {
            panic!("Unsupported architecture: {}", target);
        }
    };

    let abi = match arch {
        "arm" | "arm64" => "aapcs",
        "mips32" => "o32",
        "mips64" => "n64",
        _ => {
            if is_win {
                "ms"
            } else {
                "sysv"
            }
        }
    };

    let format = if is_win {
        "pe"
    } else if target.contains("apple") {
        "macho"
    } else if target.ends_with("aix") {
        "xcoff"
    } else {
        "elf"
    };

    let (asm, ext) = if is_win_msvc {
        if arch == "arm" {
            ("armasm", "asm")
        } else {
            ("masm", "asm")
        }
    } else if is_win_gnu {
        ("gas", "asm")
    } else {
        ("gas", "S")
    };

    let base_path: PathBuf = ["src", "asm"].iter().collect();
    let mut config = cc::Build::new();

    config.define("BOOST_CONTEXT_EXPORT", None);

    if is_win_gnu {
        config.flag("-x").flag("assembler-with-cpp");
    }

    let file_name = [arch, "_", abi, "_", format, "_", asm, ".", ext].concat();

    let path = base_path.join(file_name);
    config.file(&path);

    config.compile("libboost_context.a");

    let first_line = {
        let file = fs::File::open(path).unwrap();
        BufReader::new(file).lines().next().unwrap().unwrap()
    };
    let s = first_line.split(' ').nth(1).unwrap();
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("context_size.txt"),
        s,
    )
    .unwrap();
}
