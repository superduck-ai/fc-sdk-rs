use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;
use std::sync::OnceLock;

static IS_INTEL: OnceLock<bool> = OnceLock::new();

pub fn support_cpu_template() -> io::Result<bool> {
    #[cfg(not(target_arch = "x86_64"))]
    {
        Ok(false)
    }

    #[cfg(target_arch = "x86_64")]
    {
        if let Some(value) = IS_INTEL.get() {
            return Ok(*value);
        }

        let value = check_is_intel("/proc/cpuinfo")?;
        let _ = IS_INTEL.set(value);
        Ok(value)
    }
}

fn check_is_intel(path: impl AsRef<Path>) -> io::Result<bool> {
    let file = std::fs::File::open(path)?;
    Ok(find_first_vendor_id(file)?.as_deref() == Some("GenuineIntel"))
}

pub fn find_first_vendor_id(reader: impl Read) -> io::Result<Option<String>> {
    let reader = BufReader::new(reader);

    for line in reader.lines() {
        let line = line?;
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };

        if name.trim() == "vendor_id" {
            return Ok(Some(value.trim().to_string()));
        }
    }

    Ok(None)
}
