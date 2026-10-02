//! Phase 2 (9.4.0): deterministic, read-only Python replay environment profiles.
//!
//! spec (schema + platform + python tag + normalized packages) -> fingerprint
//! -> <cache_root>/environments/v1/python/<os>-<arch>-<tag>-<fp12>/{venv, manifest.json}
//!
//! The fingerprint is a lookup key, NOT a reproducibility proof. The manifest
//! documents what was resolved; it does not guarantee identical resolution later.

use serde::{Deserialize, Serialize};
use std::env::consts::{ARCH, OS};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const ENV_SCHEMA: u32 = 1;
pub const PROVISION_COMMAND: &str = "sel-agent provision-python-environments";
pub const PYTEST_PIN: &str = "pytest==8.1.1";

/// Environment of the scaffold path (bench --suite all Python cases).
pub const SCAFFOLD_BASE: &[&str] = &[
    "pytest==8.1.1",
    "pytest-cov==5.0.0",
    "flask",
    "fastapi",
    "uvicorn[standard]",
    "httpx",
    "requests",
];

/// Distribution name: extras and version constraints stripped, lowercased,
/// '_' normalized to '-'.
pub fn dist_name(req: &str) -> String {
    req.split(['=', '<', '>', '!', '~', '[', ' ', ';'])
        .next()
        .unwrap_or(req)
        .trim()
        .to_lowercase()
        .replace('_', "-")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonEnvSpec {
    pub python_tag: String,
    pub packages: Vec<String>,
}

impl PythonEnvSpec {
    pub fn new(python_tag: &str, packages: &[&str]) -> Self {
        let mut out: Vec<String> = Vec::new();
        for raw in packages {
            let req = raw.trim();
            if req.is_empty() {
                continue;
            }
            // The first requirement for a distribution wins (pins come first).
            if out.iter().any(|r| dist_name(r) == dist_name(req)) {
                continue;
            }
            out.push(req.to_string());
        }
        out.sort();
        Self {
            python_tag: python_tag.to_string(),
            packages: out,
        }
    }

    pub fn platform() -> String {
        format!("{OS}-{ARCH}")
    }

    pub fn canonical(&self) -> String {
        format!(
            "schema={}|platform={}|python={}|packages={}",
            ENV_SCHEMA,
            Self::platform(),
            self.python_tag,
            self.packages.join(",")
        )
    }

    pub fn fingerprint(&self) -> String {
        crate::provider_state::key_fingerprint(&self.canonical())
    }
}

/// Spec for the scaffold path.
pub fn scaffold_spec(python_tag: &str) -> PythonEnvSpec {
    PythonEnvSpec::new(python_tag, SCAFFOLD_BASE)
}

/// Spec for the scaffold path with goal-inferred extras. Extras already in the
/// base list dedupe away, so bench-all Flask/FastAPI cases share one profile.
pub fn scaffold_spec_with_extras(python_tag: &str, extras: &[&str]) -> PythonEnvSpec {
    let mut all: Vec<&str> = SCAFFOLD_BASE.to_vec();
    all.extend_from_slice(extras);
    PythonEnvSpec::new(python_tag, &all)
}

/// Spec for a benchmark case: pinned pytest + the case's declared extras only.
/// Deliberately NOT a union, so missing-dependency semantics are preserved.
pub fn case_spec(python_tag: &str, extras: &[&str]) -> PythonEnvSpec {
    let mut all = vec![PYTEST_PIN];
    all.extend_from_slice(extras);
    PythonEnvSpec::new(python_tag, &all)
}

pub fn profile_dir(cache_root: &Path, spec: &PythonEnvSpec) -> PathBuf {
    let fingerprint = spec.fingerprint();
    let hex = fingerprint
        .strip_prefix("fnv1a128:")
        .unwrap_or(&fingerprint);
    cache_root.join("environments/v1/python").join(format!(
        "{}-{}-{}",
        PythonEnvSpec::platform(),
        spec.python_tag,
        hex
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvManifest {
    pub schema: u32,
    pub fingerprint: String,
    pub platform: String,
    pub python_tag: String,
    pub packages: Vec<String>,
    /// `pip freeze` output at provisioning time (documentation, not a guarantee).
    pub resolved: Vec<String>,
}

/// Importable module name for a requirement. Distribution and import names
/// differ in general, so known mismatches are explicit rather than guessed.
pub fn import_name(req: &str) -> String {
    match dist_name(req).as_str() {
        "fastapi" => "fastapi",
        "uvicorn" => "uvicorn",
        "flask" => "flask",
        "httpx" => "httpx",
        "requests" => "requests",
        "pytest" => "pytest",
        "pytest-asyncio" => "pytest_asyncio",
        "pytest-cov" => "pytest_cov",
        other => other,
    }
    .to_string()
}

fn replay_mismatch(detail: &str) -> String {
    format!(
        "REPLAY_ENV_MISMATCH: {detail}. Run `{PROVISION_COMMAND}` to provision \
         the required Python replay environment."
    )
}

/// A profile is usable only when its venv exists and its manifest matches the
/// requested spec. The manifest is the completion marker: a directory without
/// one is treated as absent (partial or legacy), never as usable.
pub fn verify_profile(profile: &Path, spec: &PythonEnvSpec) -> Result<PathBuf, String> {
    let venv = profile.join("venv");
    if !venv.join("bin/pytest").is_file() {
        return Err(replay_mismatch(
            "cached Python venv unavailable during replay",
        ));
    }

    let text = std::fs::read_to_string(profile.join("manifest.json"))
        .map_err(|_| replay_mismatch("profile manifest is missing or unreadable"))?;
    let manifest: EnvManifest =
        serde_json::from_str(&text).map_err(|_| replay_mismatch("profile manifest is invalid"))?;

    if manifest.schema != ENV_SCHEMA
        || manifest.fingerprint != spec.fingerprint()
        || manifest.platform != PythonEnvSpec::platform()
        || manifest.python_tag != spec.python_tag
        || manifest.packages != spec.packages
    {
        return Err(replay_mismatch(
            "profile manifest does not match the requested environment",
        ));
    }

    Ok(venv)
}

/// Outcome of a provisioning attempt.
#[derive(Debug, PartialEq, Eq)]
pub enum ProvisionOutcome {
    /// A valid profile already existed; nothing was done.
    AlreadyProvisioned,
    /// A new profile was built and its manifest written.
    Provisioned,
}

/// Per-phase deadlines. Every subprocess runs through `output_with_timeout`,
/// which kills and reaps the whole process group on a deadline.
#[derive(Debug, Clone, Copy)]
pub struct ProvisionTimeouts {
    pub venv: Duration,
    pub install: Duration,
    pub import: Duration,
    pub freeze: Duration,
}

impl Default for ProvisionTimeouts {
    fn default() -> Self {
        Self {
            venv: Duration::from_secs(120),
            install: Duration::from_secs(600),
            import: Duration::from_secs(30),
            freeze: Duration::from_secs(60),
        }
    }
}

#[cfg(test)]
impl ProvisionTimeouts {
    fn fast() -> Self {
        Self {
            venv: Duration::from_secs(10),
            install: Duration::from_secs(2),
            import: Duration::from_secs(5),
            freeze: Duration::from_secs(5),
        }
    }
}

async fn run_checked(
    program: &Path,
    args: &[&str],
    cwd: &Path,
    what: &str,
    deadline: Duration,
) -> Result<std::process::Output, String> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args).current_dir(cwd).env("PYTHONNOUSERSITE", "1");
    let out = crate::executor::process::output_with_timeout(&mut cmd, deadline)
        .await
        .map_err(|e| format!("provision: failed to spawn {what}: {e}"))?
        .ok_or_else(|| {
            format!(
                "provision: {what} exceeded its deadline of {}s",
                deadline.as_secs()
            )
        })?;
    if !out.status.success() {
        return Err(format!(
            "provision: {what} failed (status {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
                .chars()
                .take(300)
                .collect::<String>()
        ));
    }
    Ok(out)
}

/// Interpreter tag ("py312") from the actual binary. The tag is part of the
/// fingerprint because a venv is not portable across Python minor versions
/// (compiled wheels are ABI-tagged).
pub fn detect_python_tag(python_bin: &Path) -> Result<String, String> {
    let out = std::process::Command::new(python_bin)
        .args([
            "-c",
            "import sys; print(f'py{sys.version_info[0]}{sys.version_info[1]}')",
        ])
        .output()
        .map_err(|e| format!("cannot run {}: {e}", python_bin.display()))?;
    if !out.status.success() {
        return Err(format!(
            "{} failed to report its version",
            python_bin.display()
        ));
    }
    let tag = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !tag.starts_with("py") || tag.len() < 4 || !tag[2..].chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("unexpected python version tag: {tag:?}"));
    }
    Ok(tag)
}

/// Lock file for a profile. Deliberately a sibling, not a child: a rebuild
/// removes the profile directory and would erase a lock stored inside it.
pub fn lock_path(cache_root: &Path, spec: &PythonEnvSpec) -> PathBuf {
    let profile = profile_dir(cache_root, spec);
    let name = profile
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("profile");
    profile
        .parent()
        .unwrap_or(cache_root)
        .join(format!(".{name}.lock"))
}

/// Build one profile in its final directory under an exclusive lock:
/// venv -> pip install -> smoke imports -> pip freeze -> manifest (atomic,
/// last) -> read-only. On any failure the directory is removed, so a reader
/// sees either a complete, manifest-verified profile or nothing.
///
/// No staging + rename: venv scripts carry absolute shebangs, so a venv must
/// be built where it will live. Network use belongs to the explicit command.
pub async fn provision_profile(
    cache_root: &Path,
    spec: &PythonEnvSpec,
    python_bin: &Path,
) -> Result<ProvisionOutcome, String> {
    provision_profile_with(cache_root, spec, python_bin, ProvisionTimeouts::default()).await
}

pub async fn provision_profile_with(
    cache_root: &Path,
    spec: &PythonEnvSpec,
    python_bin: &Path,
    timeouts: ProvisionTimeouts,
) -> Result<ProvisionOutcome, String> {
    let profile = profile_dir(cache_root, spec);

    if verify_profile(&profile, spec).is_ok() {
        return Ok(ProvisionOutcome::AlreadyProvisioned);
    }

    // Lock BEFORE anything destructive; the lock lives outside the profile.
    let lock = lock_path(cache_root, spec);
    if let Some(parent) = lock.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("provision: cannot create cache dir: {e}"))?;
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
    {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(format!(
                "provision: profile is locked by another process ({}); retry later",
                lock.display()
            ));
        }
        Err(e) => return Err(format!("provision: cannot create lock file: {e}")),
    }

    // Holding the lock: clear any incomplete or legacy profile.
    if profile.exists() {
        make_tree_writable(&profile);
        if let Err(e) = std::fs::remove_dir_all(&profile) {
            let _ = std::fs::remove_file(&lock);
            return Err(format!("provision: cannot clear partial profile: {e}"));
        }
    }
    if let Err(e) = std::fs::create_dir_all(&profile) {
        let _ = std::fs::remove_file(&lock);
        return Err(format!("provision: cannot create profile dir: {e}"));
    }

    let result = match build_profile(&profile, spec, python_bin, timeouts).await {
        Ok(()) => {
            make_tree_readonly(&profile);
            Ok(ProvisionOutcome::Provisioned)
        }
        Err(e) => {
            make_tree_writable(&profile);
            let _ = std::fs::remove_dir_all(&profile);
            Err(e)
        }
    };
    let _ = std::fs::remove_file(&lock);
    result
}

async fn build_profile(
    profile: &Path,
    spec: &PythonEnvSpec,
    python_bin: &Path,
    timeouts: ProvisionTimeouts,
) -> Result<(), String> {
    run_checked(
        python_bin,
        &["-m", "venv", "venv"],
        profile,
        "venv creation",
        timeouts.venv,
    )
    .await?;

    let venv = profile.join("venv");
    let pip = venv.join("bin/pip");
    let mut args: Vec<&str> = vec!["install", "-q"];
    args.extend(spec.packages.iter().map(String::as_str));
    run_checked(&pip, &args, profile, "pip install", timeouts.install).await?;

    let py = venv.join("bin/python3");
    for req in &spec.packages {
        let module = import_name(req);
        run_checked(
            &py,
            &["-c", &format!("import {module}")],
            profile,
            &format!("smoke import of {module}"),
            timeouts.import,
        )
        .await?;
    }

    let freeze = run_checked(&pip, &["freeze"], profile, "pip freeze", timeouts.freeze).await?;
    let resolved: Vec<String> = String::from_utf8_lossy(&freeze.stdout)
        .lines()
        .map(str::to_string)
        .collect();

    let manifest = EnvManifest {
        schema: ENV_SCHEMA,
        fingerprint: spec.fingerprint(),
        platform: PythonEnvSpec::platform(),
        python_tag: spec.python_tag.clone(),
        packages: spec.packages.clone(),
        resolved,
    };
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("provision: manifest serialize: {e}"))?;

    // Completion marker: written to a temp file, then atomically renamed.
    let tmp = profile.join(".manifest.json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("provision: manifest write: {e}"))?;
    std::fs::rename(&tmp, profile.join("manifest.json"))
        .map_err(|e| format!("provision: manifest publish: {e}"))
}

/// Distinct case specs for a list of declared extras sets (order-insensitive).
pub fn case_specs_for(python_tag: &str, extras_sets: &[Vec<&str>]) -> Vec<PythonEnvSpec> {
    let mut out: Vec<PythonEnvSpec> = Vec::new();
    for extras in extras_sets {
        let spec = case_spec(python_tag, extras);
        if !out.iter().any(|s| s.fingerprint() == spec.fingerprint()) {
            out.push(spec);
        }
    }
    out
}

/// Provision every profile the replay gate needs: the scaffold base profile plus
/// one case profile per distinct extras set. Returns (built, already_present).
/// Network use is expected here; this is the explicit provisioning command.
pub async fn provision_all(
    cache_root: &Path,
    python_bin: &Path,
    case_extras: &[Vec<&str>],
) -> Result<(usize, usize), String> {
    let tag = detect_python_tag(python_bin)?;
    let mut specs = vec![scaffold_spec(&tag)];
    specs.extend(case_specs_for(&tag, case_extras));
    let (mut built, mut present) = (0usize, 0usize);
    for spec in &specs {
        let dir = profile_dir(cache_root, spec);
        println!(
            "   profile {} -> {}",
            spec.packages.join(","),
            dir.display()
        );
        match provision_profile(cache_root, spec, python_bin).await? {
            ProvisionOutcome::Provisioned => built += 1,
            ProvisionOutcome::AlreadyProvisioned => present += 1,
        }
    }
    Ok((built, present))
}

#[cfg(unix)]
fn set_tree_mode(root: &Path, writable: bool) {
    use std::os::unix::fs::PermissionsExt;
    let entries: Vec<_> = walk(root);
    for path in entries {
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
            if meta.file_type().is_symlink() {
                continue;
            }
            let mut mode = meta.permissions().mode();
            if writable {
                mode |= 0o200;
            } else {
                mode &= !0o222;
            }
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode));
        }
    }
}

fn walk(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                out.push(path.clone());
                if path.is_dir() && !path.is_symlink() {
                    stack.push(path);
                }
            }
        }
    }
    out
}

fn make_tree_readonly(root: &Path) {
    #[cfg(unix)]
    set_tree_mode(root, false);
}

fn make_tree_writable(root: &Path) {
    #[cfg(unix)]
    set_tree_mode(root, true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const TAG: &str = "py312";

    fn block_on<F: std::future::Future>(fut: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
            .block_on(fut)
    }

    fn provision_sync(
        root: &Path,
        spec: &PythonEnvSpec,
        py: &Path,
    ) -> Result<ProvisionOutcome, String> {
        block_on(provision_profile(root, spec, py))
    }

    fn provision_sync_fast(
        root: &Path,
        spec: &PythonEnvSpec,
        py: &Path,
    ) -> Result<ProvisionOutcome, String> {
        block_on(provision_profile_with(
            root,
            spec,
            py,
            ProvisionTimeouts::fast(),
        ))
    }

    fn fake_profile(root: &Path, spec: &PythonEnvSpec, manifest: Option<EnvManifest>) -> PathBuf {
        let dir = profile_dir(root, spec);
        std::fs::create_dir_all(dir.join("venv/bin")).expect("setup");
        std::fs::write(dir.join("venv/bin/pytest"), b"fixture").expect("setup");
        if let Some(m) = manifest {
            std::fs::write(
                dir.join("manifest.json"),
                serde_json::to_vec(&m).expect("json"),
            )
            .expect("setup");
        }
        dir
    }

    fn good_manifest(spec: &PythonEnvSpec) -> EnvManifest {
        EnvManifest {
            schema: ENV_SCHEMA,
            fingerprint: spec.fingerprint(),
            platform: PythonEnvSpec::platform(),
            python_tag: spec.python_tag.clone(),
            packages: spec.packages.clone(),
            resolved: vec![],
        }
    }

    // ---- pure logic (new, expected green) ----

    #[test]
    fn fingerprint_is_deterministic() {
        assert_eq!(
            case_spec(TAG, &["requests"]).fingerprint(),
            case_spec(TAG, &["requests"]).fingerprint()
        );
    }

    #[test]
    fn fingerprint_ignores_extra_order() {
        assert_eq!(
            case_spec(TAG, &["requests", "pytest-asyncio"]).fingerprint(),
            case_spec(TAG, &["pytest-asyncio", "requests"]).fingerprint()
        );
    }

    #[test]
    fn fingerprint_changes_with_extras() {
        assert_ne!(
            case_spec(TAG, &["requests"]).fingerprint(),
            case_spec(TAG, &["pytest-asyncio"]).fingerprint()
        );
    }

    #[test]
    fn case_spec_is_not_a_union_and_dedups_pytest() {
        // PY-05 declares ["requests", "pytest"].
        let s = case_spec(TAG, &["requests", "pytest"]);
        assert_eq!(s.packages, vec!["pytest==8.1.1", "requests"]);
        assert!(!s.packages.iter().any(|p| dist_name(p) == "flask"));
    }

    #[test]
    fn import_name_strips_extras() {
        assert_eq!(import_name("uvicorn[standard]"), "uvicorn");
    }

    #[test]
    fn guard_valid_fake_profile_is_accepted() {
        let root = tempdir().expect("root");
        let spec = case_spec(TAG, &["requests"]);
        let dir = fake_profile(root.path(), &spec, Some(good_manifest(&spec)));
        assert_eq!(
            verify_profile(&dir, &spec).expect("accepted"),
            dir.join("venv")
        );
    }

    // ---- contract (red against current behaviour) ----

    #[test]
    fn bug_import_name_maps_pytest_asyncio() {
        assert_eq!(import_name("pytest-asyncio"), "pytest_asyncio");
    }

    #[test]
    fn bug_profile_without_manifest_is_rejected() {
        let root = tempdir().expect("root");
        let spec = case_spec(TAG, &[]);
        let dir = fake_profile(root.path(), &spec, None);
        let err = verify_profile(&dir, &spec).expect_err("no manifest => invalid");
        assert!(err.contains("REPLAY_ENV_MISMATCH"));
    }

    #[test]
    fn bug_profile_with_foreign_fingerprint_is_rejected() {
        let root = tempdir().expect("root");
        let spec = case_spec(TAG, &[]);
        let mut m = good_manifest(&spec);
        m.fingerprint = "0000deadbeef".to_string();
        let dir = fake_profile(root.path(), &spec, Some(m));
        assert!(verify_profile(&dir, &spec).is_err());
    }

    #[test]
    fn bug_missing_profile_error_names_the_provision_command() {
        let root = tempdir().expect("root");
        let spec = case_spec(TAG, &[]);
        let err =
            verify_profile(&profile_dir(root.path(), &spec), &spec).expect_err("missing profile");
        assert!(err.contains("REPLAY_ENV_MISMATCH"));
        assert!(err.contains(PROVISION_COMMAND), "not actionable: {err}");
    }

    #[test]
    fn profile_dir_uses_full_hex_fingerprint_without_colon() {
        let spec = case_spec(TAG, &["requests"]);
        let dir = profile_dir(Path::new("/cache"), &spec);
        assert!(dir.starts_with("/cache/environments/v1/python"));
        let name = dir.file_name().and_then(|n| n.to_str()).expect("name");
        assert!(!name.contains(':'), "colon in dir name: {name}");
        let hex = name.rsplit('-').next().expect("hex part");
        assert_eq!(hex.len(), 32, "fingerprint part: {hex}");
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ---- provisioner (hermetic: scripted python, no network, no HOME) ----

    /// A fake python3: creates venv layout with pip/python3 scripts that
    /// succeed for install/freeze/-c imports. Everything stays in tempdirs.
    fn write_fake_python(dir: &Path) -> PathBuf {
        let bin = dir.join("fake-python3");
        let script = r##"#!/bin/sh
# fake python3: only supports "-m venv venv"
if [ "$1" = "-m" ] && [ "$2" = "venv" ]; then
  mkdir -p "$3/bin"
  cat > "$3/bin/pip" <<'EOF'
#!/bin/sh
if [ "$1" = "freeze" ]; then echo "pytest==8.1.1"; fi
exit 0
EOF
  cat > "$3/bin/python3" <<'EOF'
#!/bin/sh
exit 0
EOF
  cat > "$3/bin/pytest" <<'EOF'
#!/bin/sh
exit 0
EOF
  chmod +x "$3/bin/pip" "$3/bin/python3" "$3/bin/pytest"
  exit 0
fi
exit 1
"##;
        std::fs::write(&bin, script).expect("write fake python");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("chmod fake python");
        }
        bin
    }

    #[test]
    fn provision_builds_profile_and_verify_accepts_it() {
        let root = tempdir().expect("root");
        let tools = tempdir().expect("tools");
        let py = write_fake_python(tools.path());
        let spec = case_spec(TAG, &[]);

        let out = provision_sync(root.path(), &spec, &py).expect("provision");
        assert_eq!(out, ProvisionOutcome::Provisioned);
        verify_profile(&profile_dir(root.path(), &spec), &spec).expect("verified");
    }

    #[test]
    fn provision_is_idempotent() {
        let root = tempdir().expect("root");
        let tools = tempdir().expect("tools");
        let py = write_fake_python(tools.path());
        let spec = case_spec(TAG, &[]);

        provision_sync(root.path(), &spec, &py).expect("first");
        let second = provision_sync(root.path(), &spec, &py).expect("second");
        assert_eq!(second, ProvisionOutcome::AlreadyProvisioned);
    }

    #[test]
    fn provision_rebuilds_partial_profile_without_manifest() {
        let root = tempdir().expect("root");
        let tools = tempdir().expect("tools");
        let py = write_fake_python(tools.path());
        let spec = case_spec(TAG, &[]);

        // Partial: venv exists, no manifest (e.g. interrupted provisioning).
        let dir = profile_dir(root.path(), &spec);
        std::fs::create_dir_all(dir.join("venv/bin")).expect("partial");
        std::fs::write(dir.join("venv/bin/pytest"), b"stale").expect("partial");

        let out = provision_sync(root.path(), &spec, &py).expect("rebuild");
        assert_eq!(out, ProvisionOutcome::Provisioned);
        verify_profile(&dir, &spec).expect("valid after rebuild");
    }

    #[cfg(unix)]
    #[test]
    fn provisioned_profile_is_read_only() {
        let root = tempdir().expect("root");
        let tools = tempdir().expect("tools");
        let py = write_fake_python(tools.path());
        let spec = case_spec(TAG, &[]);

        provision_sync(root.path(), &spec, &py).expect("provision");
        let dir = profile_dir(root.path(), &spec);
        let err = std::fs::write(dir.join("venv/injected.txt"), b"x");
        assert!(err.is_err(), "profile venv must not be writable");
        // Cleanup so tempdir can be removed.
        make_tree_writable(&dir);
    }

    #[test]
    fn provision_fails_cleanly_when_python_is_broken() {
        let root = tempdir().expect("root");
        let tools = tempdir().expect("tools");
        let bad = tools.path().join("broken-python");
        std::fs::write(&bad, "#!/bin/sh\nexit 7\n").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bad, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        let spec = case_spec(TAG, &[]);

        let err = provision_sync(root.path(), &spec, &bad).expect_err("must fail");
        assert!(err.contains("venv creation"), "unexpected error: {err}");
        // No manifest => profile is NOT usable.
        assert!(verify_profile(&profile_dir(root.path(), &spec), &spec).is_err());
    }

    // ---- lock ordering (red before fix) ----

    #[test]
    fn bug_lock_is_taken_before_destroying_existing_profile() {
        let root = tempdir().expect("root");
        let tools = tempdir().expect("tools");
        let py = write_fake_python(tools.path());
        let spec = case_spec(TAG, &[]);

        // A concurrent provisioner holds the lock.
        let dir = profile_dir(root.path(), &spec);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(lock_path(root.path(), &spec), b"held").expect("lock");

        // Existing partial content that must survive a refused attempt.
        std::fs::create_dir_all(dir.join("venv/bin")).expect("partial");
        std::fs::write(dir.join("venv/bin/marker"), b"keep").expect("marker");

        let err = provision_sync(root.path(), &spec, &py).expect_err("must refuse while locked");
        assert!(err.contains("locked"), "unexpected: {err}");
        assert!(
            dir.join("venv/bin/marker").exists(),
            "a refused attempt must not destroy the other provisioner's work"
        );
    }

    #[test]
    fn bug_lock_lives_outside_the_profile_directory() {
        let root = tempdir().expect("root");
        let spec = case_spec(TAG, &[]);
        let dir = profile_dir(root.path(), &spec);
        let lock = lock_path(root.path(), &spec);
        assert!(
            !lock.starts_with(&dir),
            "lock inside the profile is erased by rebuild: {}",
            lock.display()
        );
    }

    // ---- interpreter tag ----

    #[cfg(unix)]
    fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, body).expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod script");
        path
    }

    #[cfg(unix)]
    #[test]
    fn detect_python_tag_reads_the_interpreter() {
        let tools = tempdir().expect("tools");
        let py = write_script(tools.path(), "py", "#!/bin/sh\necho py312\n");
        assert_eq!(detect_python_tag(&py).expect("tag"), "py312");
    }

    #[cfg(unix)]
    #[test]
    fn detect_python_tag_rejects_garbage_output() {
        let tools = tempdir().expect("tools");
        let py = write_script(tools.path(), "py", "#!/bin/sh\necho not-a-tag\n");
        assert!(detect_python_tag(&py).is_err());
    }

    #[test]
    fn detect_python_tag_reports_missing_interpreter() {
        assert!(detect_python_tag(Path::new("/nonexistent/python3")).is_err());
    }

    // ---- atomic publication (red before staging+rename) ----

    /// A python that creates the venv layout, then HANGS on pip install.
    /// Simulates an interrupted provisioning run.
    #[cfg(unix)]
    fn write_slow_python(dir: &Path) -> PathBuf {
        let bin = dir.join("slow-python3");
        let script = r##"#!/bin/sh
if [ "$1" = "-m" ] && [ "$2" = "venv" ]; then
  mkdir -p "$3/bin"
  cat > "$3/bin/pip" <<'EOF'
#!/bin/sh
if [ "$1" = "freeze" ]; then echo "pytest==8.1.1"; exit 0; fi
# install hangs forever
sleep 3600
EOF
  cat > "$3/bin/python3" <<'EOF'
#!/bin/sh
exit 0
EOF
  cat > "$3/bin/pytest" <<'EOF'
#!/bin/sh
exit 0
EOF
  chmod +x "$3/bin/pip" "$3/bin/python3" "$3/bin/pytest"
  exit 0
fi
exit 1
"##;
        std::fs::write(&bin, script).expect("write slow python");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        bin
    }

    #[cfg(unix)]
    #[test]
    fn bug_provision_has_a_deadline() {
        let root = tempdir().expect("root");
        let tools = tempdir().expect("tools");
        let py = write_slow_python(tools.path());
        let spec = case_spec(TAG, &[]);

        let start = std::time::Instant::now();
        let result = provision_sync_fast(root.path(), &spec, &py);
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(60),
            "provision must have a deadline; ran for {elapsed:?}"
        );
        assert!(result.is_err(), "a hung install must fail, not succeed");
    }

    #[cfg(unix)]
    #[test]
    fn bug_failed_provision_publishes_nothing_at_the_final_path() {
        let root = tempdir().expect("root");
        let tools = tempdir().expect("tools");
        let bad = tools.path().join("fails-after-venv");
        // venv succeeds, pip install fails => partial tree inside final path.
        let script = r##"#!/bin/sh
if [ "$1" = "-m" ] && [ "$2" = "venv" ]; then
  mkdir -p "$3/bin"
  cat > "$3/bin/pip" <<'EOF'
#!/bin/sh
exit 9
EOF
  cat > "$3/bin/pytest" <<'EOF'
#!/bin/sh
exit 0
EOF
  chmod +x "$3/bin/pip" "$3/bin/pytest"
  exit 0
fi
exit 1
"##;
        std::fs::write(&bad, script).expect("write");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bad, std::fs::Permissions::from_mode(0o755)).expect("chmod");

        let spec = case_spec(TAG, &[]);
        let err = provision_sync(root.path(), &spec, &bad).expect_err("must fail");
        assert!(err.contains("pip install"), "unexpected: {err}");

        // The contract: a reader sees a complete profile or nothing at all.
        let dir = profile_dir(root.path(), &spec);
        assert!(
            !dir.exists(),
            "failed provisioning must not leave a partial profile at {}",
            dir.display()
        );
    }
}
