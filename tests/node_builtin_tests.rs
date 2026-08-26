// tests/node_builtin_tests.rs
// اختبارات A3 — Node.js built-in guard
// cargo test --test node_builtin_tests -- --nocapture

// ملاحظة: هذه الاختبارات تفترض وجود src/executor/node_builtins.rs
// شغّل بعد إنشاء الـ module في A3

use sel_agent::executor::node_builtins::{
    extract_npm_package, is_node_builtin, is_npm_install_builtin, is_pip_without_package,
};

// ─── is_node_builtin ───────────────────────────────────────────────────────

#[test]
fn crypto_is_builtin() {
    assert!(
        is_node_builtin("crypto"),
        "crypto must be recognized as Node.js built-in"
    );
}

#[test]
fn path_is_builtin() {
    assert!(is_node_builtin("path"));
}

#[test]
fn fs_is_builtin() {
    assert!(is_node_builtin("fs"));
}

#[test]
fn os_is_builtin() {
    assert!(is_node_builtin("os"));
}

#[test]
fn stream_is_builtin() {
    assert!(is_node_builtin("stream"));
}

#[test]
fn http_is_builtin() {
    assert!(is_node_builtin("http"));
}

#[test]
fn https_is_builtin() {
    assert!(is_node_builtin("https"));
}

#[test]
fn events_is_builtin() {
    assert!(is_node_builtin("events"));
}

#[test]
fn util_is_builtin() {
    assert!(is_node_builtin("util"));
}

#[test]
fn child_process_is_builtin() {
    assert!(is_node_builtin("child_process"));
}

#[test]
fn node_prefix_stripped_correctly() {
    // node:crypto → crypto → built-in
    assert!(is_node_builtin("node:crypto"));
    assert!(is_node_builtin("node:fs"));
    assert!(is_node_builtin("node:path"));
}

#[test]
fn fs_promises_subpath_recognized() {
    // fs/promises → base = fs → built-in
    assert!(is_node_builtin("fs/promises"));
}

#[test]
fn timers_promises_subpath_recognized() {
    assert!(is_node_builtin("timers/promises"));
}

#[test]
fn express_is_not_builtin() {
    assert!(
        !is_node_builtin("express"),
        "express is an npm package, not built-in"
    );
}

#[test]
fn lodash_is_not_builtin() {
    assert!(!is_node_builtin("lodash"));
}

#[test]
fn axios_is_not_builtin() {
    assert!(!is_node_builtin("axios"));
}

#[test]
fn react_is_not_builtin() {
    assert!(!is_node_builtin("react"));
}

#[test]
fn uuid_is_not_builtin() {
    assert!(
        !is_node_builtin("uuid"),
        "uuid is an npm package — TS-01 الخطأ الأصلي"
    );
}

#[test]
fn typescript_is_not_builtin() {
    assert!(!is_node_builtin("typescript"));
}

#[test]
fn empty_string_is_not_builtin() {
    assert!(!is_node_builtin(""));
}

// ─── extract_npm_package ───────────────────────────────────────────────────

#[test]
fn extract_simple_package_name() {
    assert_eq!(extract_npm_package("npm install crypto"), Some("crypto"));
}

#[test]
fn extract_with_save_flag() {
    assert_eq!(
        extract_npm_package("npm install --save express"),
        Some("express")
    );
}

#[test]
fn extract_with_dev_flag() {
    assert_eq!(
        extract_npm_package("npm install --save-dev jest"),
        Some("jest")
    );
}

#[test]
fn extract_short_i_alias() {
    assert_eq!(extract_npm_package("npm i lodash"), Some("lodash"));
}

#[test]
fn extract_no_package_returns_none() {
    assert_eq!(extract_npm_package("npm install"), None);
}

#[test]
fn extract_flags_only_returns_none() {
    assert_eq!(extract_npm_package("npm install --legacy-peer-deps"), None);
}

#[test]
fn extract_non_npm_command_returns_none() {
    assert_eq!(extract_npm_package("pip install requests"), None);
    assert_eq!(extract_npm_package("yarn add express"), None);
}

// ─── is_npm_install_builtin ────────────────────────────────────────────────

#[test]
fn detect_npm_install_crypto() {
    // هذه كانت المشكلة الجذرية في TS-01
    assert_eq!(
        is_npm_install_builtin("npm install crypto"),
        Some("crypto"),
        "npm install crypto يجب أن يُرفض"
    );
}

#[test]
fn detect_npm_install_node_prefix() {
    assert_eq!(
        is_npm_install_builtin("npm install node:crypto"),
        Some("crypto")
    );
}

#[test]
fn detect_npm_install_path() {
    assert_eq!(is_npm_install_builtin("npm install path"), Some("path"));
}

#[test]
fn detect_npm_install_fs() {
    assert_eq!(is_npm_install_builtin("npm install fs"), Some("fs"));
}

#[test]
fn npm_install_express_not_blocked() {
    // express ليس built-in → يجب السماح به
    assert_eq!(
        is_npm_install_builtin("npm install express"),
        None,
        "express هو npm package حقيقي، لا يُرفض"
    );
}

#[test]
fn npm_install_uuid_not_blocked() {
    // uuid ليس built-in — هذا كان يتسبب في QuickFix خاطئ
    assert_eq!(is_npm_install_builtin("npm install uuid"), None);
}

#[test]
fn npm_install_without_package_not_matched() {
    // هذا يُعالَج بـ guard منفصل
    assert_eq!(is_npm_install_builtin("npm install"), None);
}

#[test]
fn pip_command_not_matched() {
    assert_eq!(is_npm_install_builtin("pip install crypto"), None);
}

// ─── is_pip_without_package ────────────────────────────────────────────────

#[test]
fn pip_install_without_package_detected() {
    // هذا كان يُطلق repair في PY-03 و PY-04
    assert!(is_pip_without_package("venv/bin/pip install"));
    assert!(is_pip_without_package("pip install"));
    assert!(is_pip_without_package("pip3 install"));
}

#[test]
fn pip_install_with_package_ok() {
    assert!(!is_pip_without_package("venv/bin/pip install pytest"));
    assert!(!is_pip_without_package("pip install requests"));
    assert!(!is_pip_without_package("pip3 install flask"));
}

#[test]
fn pip_install_with_flags_only_detected() {
    // فلاق بدون package → مرفوض
    assert!(is_pip_without_package("venv/bin/pip install --upgrade"));
    assert!(is_pip_without_package("pip install -q"));
}

#[test]
fn pip_install_with_flags_and_package_ok() {
    // فلاق + package → مسموح
    assert!(!is_pip_without_package("pip install --upgrade pytest"));
    assert!(!is_pip_without_package("venv/bin/pip install -q requests"));
}

#[test]
fn non_pip_command_not_affected() {
    assert!(!is_pip_without_package("npm install express"));
    assert!(!is_pip_without_package("cargo install"));
    assert!(!is_pip_without_package("apt install python3"));
}

// ─── integration: safety_check blocks built-in installs ────────────────────

#[test]
fn all_common_builtins_recognized() {
    // تأكيد أن القائمة تغطي الوحدات الشائعة
    let common = [
        "fs",
        "path",
        "crypto",
        "os",
        "http",
        "https",
        "net",
        "url",
        "util",
        "events",
        "stream",
        "buffer",
        "readline",
        "child_process",
        "worker_threads",
        "assert",
        "zlib",
    ];
    for module in &common {
        assert!(
            is_node_builtin(module),
            "'{module}' should be recognized as Node.js built-in"
        );
    }
}

#[test]
fn common_npm_packages_not_blocked() {
    // تأكيد أن الحزم الشائعة لا تُحجب
    let packages = [
        "express",
        "lodash",
        "axios",
        "react",
        "vue",
        "jest",
        "typescript",
        "uuid",
        "moment",
        "dayjs",
        "zod",
        "prisma",
        "dotenv",
        "cors",
        "body-parser",
    ];
    for pkg in &packages {
        assert!(
            !is_node_builtin(pkg),
            "'{pkg}' should NOT be blocked — it's a real npm package"
        );
    }
}
