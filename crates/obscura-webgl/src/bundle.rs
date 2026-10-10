//! Only load the verified, explicitly installed graphics bundle.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub const ANGLE_COMMIT: &str = "3796c6cb739503fba75ebc416a3c6b55c2fce783";
pub const SWIFTSHADER_COMMIT: &str = "1e80438d2b93ef36a7c05f8d2b81233bac0e3d16";

#[derive(Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub angle_commit: String,
    pub swiftshader_commit: String,
    pub os: String,
    pub arch: String,
    pub files: BTreeMap<String, String>,
}

pub struct Bundle {
    pub directory: PathBuf,
    manifest: Manifest,
}

impl Bundle {
    pub fn discover() -> Result<Self, String> {
        let directory = match std::env::var_os("OBSCURA_WEBGL_LIB_DIR") {
            Some(path) => PathBuf::from(path),
            None => std::env::current_exe()
                .map_err(|e| e.to_string())?
                .parent()
                .ok_or("executable has no parent directory")?
                .join("webgl"),
        };
        Self::open(&directory)
    }
    pub fn open(directory: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(directory.join("bundle.json"))
            .map_err(|e| format!("graphics bundle manifest unavailable: {e}"))?;
        let manifest: Manifest = serde_json::from_slice(&bytes)
            .map_err(|e| format!("invalid graphics bundle manifest: {e}"))?;
        if manifest.schema != 1
            || manifest.angle_commit != ANGLE_COMMIT
            || manifest.swiftshader_commit != SWIFTSHADER_COMMIT
        {
            return Err("graphics bundle dependency pins do not match this binary".into());
        }
        if manifest.os != std::env::consts::OS || manifest.arch != std::env::consts::ARCH {
            return Err("graphics bundle platform does not match this binary".into());
        }
        let directory = directory.canonicalize().map_err(|e| e.to_string())?;
        Ok(Self {
            directory,
            manifest,
        })
    }
    pub fn verify_vulkan_loader(&self) -> Result<(), String> {
        self.library("libvulkan.so.1")?;
        // ANGLE tries the unversioned name first. A locally installed alias
        // must not bypass verification of the mandatory versioned library.
        if self.directory.join("libvulkan.so").exists() {
            self.library("libvulkan.so")?;
        }
        Ok(())
    }
    pub fn library(&self, name: &str) -> Result<PathBuf, String> {
        // The caller supplies a fixed basename. Never honor manifest paths or
        // search the working directory/system library path for substitute GL.
        if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
            return Err("graphics library must be a basename".into());
        }
        let expected = self
            .manifest
            .files
            .get(name)
            .ok_or_else(|| format!("bundle omits {name}"))?;
        let path = self
            .directory
            .join(name)
            .canonicalize()
            .map_err(|e| format!("{name}: {e}"))?;
        if path.parent() != Some(self.directory.as_path()) {
            return Err(format!("{name} escapes the graphics bundle"));
        }
        let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        let mut digest = Sha256::new();
        std::io::copy(&mut file, &mut digest).map_err(|e| e.to_string())?;
        if format!("{:x}", digest.finalize()) != *expected {
            return Err(format!("checksum mismatch for {name}"));
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> Fixture {
        let path = std::env::temp_dir().join(format!(
            "obscura-webgl-bundle-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("driver"), b"fixture bytes").unwrap();
        let manifest = serde_json::json!({"schema":1,"angle_commit":ANGLE_COMMIT,"swiftshader_commit":SWIFTSHADER_COMMIT,
            "os":std::env::consts::OS,"arch":std::env::consts::ARCH,
            "files":{"driver":format!("{:x}",Sha256::digest(b"fixture bytes"))}});
        std::fs::write(
            path.join("bundle.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        Fixture(path)
    }
    #[test]
    fn matching_pinned_bundle_verifies_without_loading_a_library() {
        let f = fixture();
        let bundle = Bundle::open(&f.0).unwrap();
        assert_eq!(
            bundle.library("driver").unwrap(),
            f.0.canonicalize().unwrap().join("driver")
        );
    }
    #[test]
    fn missing_corrupt_and_unlisted_libraries_are_errors() {
        let f = fixture();
        let bundle = Bundle::open(&f.0).unwrap();
        assert!(bundle.library("missing").is_err());
        std::fs::write(f.0.join("driver"), b"modified").unwrap();
        assert!(bundle.library("driver").unwrap_err().contains("checksum"));
        std::fs::remove_file(f.0.join("driver")).unwrap();
        assert!(bundle.library("driver").is_err());
    }
    #[test]
    fn wrong_revision_platform_schema_and_invalid_manifest_are_errors() {
        for (key, value) in [
            ("angle_commit", serde_json::json!("wrong")),
            ("swiftshader_commit", serde_json::json!("wrong")),
            ("schema", serde_json::json!(2)),
            ("os", serde_json::json!("other")),
            ("arch", serde_json::json!("other")),
        ] {
            let f = fixture();
            let p = f.0.join("bundle.json");
            let mut m: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
            m[key] = value;
            std::fs::write(&p, serde_json::to_vec(&m).unwrap()).unwrap();
            assert!(Bundle::open(&f.0).is_err());
        }
        let f = fixture();
        std::fs::write(f.0.join("bundle.json"), b"bad json").unwrap();
        assert!(Bundle::open(&f.0).is_err());
        std::fs::remove_file(f.0.join("bundle.json")).unwrap();
        assert!(Bundle::open(&f.0).is_err());
    }
    #[test]
    fn vulkan_loader_and_any_priority_alias_are_verified_before_native_loading() {
        let f = fixture();
        assert!(Bundle::open(&f.0).unwrap().verify_vulkan_loader().unwrap_err().contains("omits"));
        let manifest_path = f.0.join("bundle.json");
        let mut manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        let digest = format!("{:x}",Sha256::digest(b"pinned loader"));
        manifest["files"]["libvulkan.so.1"] = serde_json::json!(digest);
        std::fs::write(&manifest_path,serde_json::to_vec(&manifest).unwrap()).unwrap();
        std::fs::write(f.0.join("libvulkan.so.1"),b"pinned loader").unwrap();
        assert!(Bundle::open(&f.0).unwrap().verify_vulkan_loader().is_ok());
        std::fs::write(f.0.join("libvulkan.so.1"),b"corrupt loader").unwrap();
        assert!(Bundle::open(&f.0).unwrap().verify_vulkan_loader().unwrap_err().contains("checksum"));
        std::fs::write(f.0.join("libvulkan.so.1"),b"pinned loader").unwrap();
        std::fs::write(f.0.join("libvulkan.so"),b"pinned loader").unwrap();
        assert!(Bundle::open(&f.0).unwrap().verify_vulkan_loader().unwrap_err().contains("omits"));
        manifest["files"]["libvulkan.so"] = serde_json::json!(digest);
        std::fs::write(&manifest_path,serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(Bundle::open(&f.0).unwrap().verify_vulkan_loader().is_ok());
        std::fs::write(f.0.join("libvulkan.so"),b"corrupt alias").unwrap();
        assert!(Bundle::open(&f.0).unwrap().verify_vulkan_loader().unwrap_err().contains("checksum"));
    }
    #[test]
    fn paths_cannot_escape_bundle() {
        let f = fixture();
        let bundle = Bundle::open(&f.0).unwrap();
        for name in ["", ".", "..", "../driver", "/driver", "x\\driver"] {
            assert!(bundle.library(name).is_err());
        }
    }
}
