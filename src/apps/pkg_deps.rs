//! minpkg bağımlılık çözümleme (dagitim öncesi surum/çevre analizi).
//!
//! Bu modül çekirdekten bağımsızdır (`no_std` + `alloc`); hiçbir VFS/ATA/ağ
//! çağrısı yapmaz. Girdisi depo kataloğun tamamı ve kurulu paket listesidir,
//! çıktısı ise kurulması gereken paketlerin bağımlılık sırasına göre listesidir
//! ya da neden kurulamadığını açıklayan bir hata.
//!
//! Kural kümesi (minpkg sürüm 1):
//! - `name op version` üçlüsü; `op` ∈ `=`, `>=`, `>` , `<=`, `<`
//! - Sürümler `MAJOR[.MINOR[.PATCH]]` biçiminde sayısal parçalardan oluşur,
//!   eksik parçalar 0 sayılır (`1.2` == `1.2.0`).
//! - Katalogda bulunmayan bağımlılık `MissingDependency` hatasıdır.
//! - Kurulu sürüm kuraldan buyuk/küçük uyumsuzsa `VersionMismatch` hatasıdır.
//! - Aynı ada sahip iki farklı sürüm aynı çözüm içinde istenirse `Conflict`.
//! - A → B → A gibi döngüler `Cycle` hatasıdır (sonsuz döngü yoktur).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Bir bağımlılık kuralı: paket adı + sürüm karşılaştırması.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Dependency {
    pub name: String,
    pub op: VersionOp,
    pub version: String,
}

/// Sürüm karşılaştırma operatörü.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VersionOp {
    Eq,
    Ge,
    Gt,
    Le,
    Lt,
}

impl VersionOp {
    /// `"pkg>=1.2"` biçimindeki kural metnini ayrıştır.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "=" | "==" => Some(VersionOp::Eq),
            ">=" => Some(VersionOp::Ge),
            ">" => Some(VersionOp::Gt),
            "<=" => Some(VersionOp::Le),
            "<" => Some(VersionOp::Lt),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            VersionOp::Eq => "=",
            VersionOp::Ge => ">=",
            VersionOp::Gt => ">",
            VersionOp::Le => "<=",
            VersionOp::Lt => "<",
        }
    }

    /// `candidate` sürümünü `required` sürümüne göre kurala uyup uymadığını döner.
    pub fn accepts(self, candidate: &str, required: &str) -> bool {
        let Some(c) = parse_version(candidate) else {
            return false;
        };
        let Some(r) = parse_version(required) else {
            return false;
        };
        let ord = c.cmp(&r);
        match self {
            VersionOp::Eq => ord == core::cmp::Ordering::Equal,
            VersionOp::Ge => ord != core::cmp::Ordering::Less,
            VersionOp::Gt => ord == core::cmp::Ordering::Greater,
            VersionOp::Le => ord != core::cmp::Ordering::Greater,
            VersionOp::Lt => ord == core::cmp::Ordering::Less,
        }
    }
}

impl Dependency {
    /// `"ssl-lite>=1.2.0"` biçimindeki kural satırını ayrıştır.
    /// Boşluklu (`"ssl-lite >= 1.2.0"`) biçim de kabul edilir.
    pub fn parse(spec: &str) -> Option<Self> {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return None;
        }
        // En uzun operatörten kısaya doğru ara (>= önce > gelmeli).
        for op_text in [">=", "<=", "==", "=", ">", "<"] {
            if let Some(idx) = trimmed.find(op_text) {
                let name = trimmed[..idx].trim();
                let version = trimmed[idx + op_text.len()..].trim();
                if name.is_empty() || version.is_empty() {
                    return None;
                }
                let op = VersionOp::parse(op_text)?;
                return Some(Dependency {
                    name: name.to_string(),
                    op,
                    version: version.to_string(),
                });
            }
        }
        // Operatör yoksa "kurulu olsun yeter" anlamında `>=0` kuralı.
        Some(Dependency {
            name: trimmed.to_string(),
            op: VersionOp::Ge,
            version: "0".to_string(),
        })
    }

    pub fn matches(&self, candidate: &str) -> bool {
        self.op.accepts(candidate, &self.version)
    }

    pub fn describe(&self) -> String {
        format!("{}{}{}", self.name, self.op.as_str(), self.version)
    }
}

/// Depodaki bir paket kaydı.
#[derive(Clone, Debug)]
pub struct PkgEntry {
    pub name: String,
    pub version: String,
    pub deps: Vec<Dependency>,
}

impl PkgEntry {
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            deps: Vec::new(),
        }
    }

    pub fn with_deps(mut self, deps: &[&str]) -> Self {
        for d in deps {
            if let Some(dep) = Dependency::parse(d) {
                self.deps.push(dep);
            }
        }
        self
    }
}

/// Kurulum planı: bağımlılıklar önce, istenen paket en sonda.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallPlan {
    /// Kurulacak (henüz kurulu olmayan) paketler, bağımlılık sırasıyla.
    pub to_install: Vec<String>,
    /// Zaten kurulu olduğu için atlanan paketler ve sürümleri.
    pub already_installed: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveError {
    /// İstenen paket katalogda yok.
    UnknownPackage(String),
    /// Katalogda birden fazla sürüm var ve hiçbiri bağımlılık kuralını sağlamıyor.
    UnsatisfiedVersion { package: String, required: String },
    /// Bağımlılık katalogda yok.
    MissingDependency { package: String, missing: String },
    /// Kurulu sürüm bağımlılık kuralını sağlamıyor (otomatik yükseltme yapılmaz).
    VersionMismatch {
        package: String,
        installed: String,
        required: String,
    },
    /// İki paket aynı bağımlılık için farklı sürüm istiyor.
    Conflict {
        package: String,
        wanted: String,
        also_wanted: String,
    },
    /// Bağımlılık döngüsü.
    Cycle { chain: String },
}

impl ResolveError {
    /// Komut satırı / GUI için okunabilir (ASCII) hata metni.
    pub fn message(&self) -> String {
        match self {
            ResolveError::UnknownPackage(p) => format!("Paket katalogda yok: {}", p),
            ResolveError::UnsatisfiedVersion { package, required } => format!(
                "{} paketi için kuralı sağlayan sürüm katalogda yok: {}",
                package, required
            ),
            ResolveError::MissingDependency { package, missing } => {
                format!(
                    "{} paketinin bağımlılığı katalogda yok: {}",
                    package, missing
                )
            }
            ResolveError::VersionMismatch {
                package,
                installed,
                required,
            } => format!(
                "{} paketinin kurulu sürümü ({}) bağımlılık kuralını sağlamıyor: {}",
                package, installed, required
            ),
            ResolveError::Conflict {
                package,
                wanted,
                also_wanted,
            } => format!(
                "Çakışma: {} için iki farklı sürüm istendi ({} / {})",
                package, wanted, also_wanted
            ),
            ResolveError::Cycle { chain } => format!("Bağımlılık döngüsü: {}", chain),
        }
    }
}

/// Sürümü sayısal parçalara ayır; numerik olmayan parçada ayrıştırma biter.
pub fn parse_version(v: &str) -> Option<[u32; 3]> {
    let clean = v.trim();
    if clean.is_empty() {
        return None;
    }
    let mut out = [0u32; 3];
    let mut idx = 0usize;
    for part in clean.split('.') {
        if idx >= 3 {
            break;
        }
        // "1.2-beta" gibi ekleri yok say (ilk rakam dizisi yeterli).
        let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            if idx == 0 {
                return None;
            }
            break;
        }
        out[idx] = digits.parse::<u32>().unwrap_or(0);
        idx += 1;
    }
    if idx == 0 {
        None
    } else {
        Some(out)
    }
}

pub fn compare_version(a: &str, b: &str) -> core::cmp::Ordering {
    match (parse_version(a), parse_version(b)) {
        (Some(pa), Some(pb)) => pa.cmp(&pb),
        (Some(_), None) => core::cmp::Ordering::Greater,
        (None, Some(_)) => core::cmp::Ordering::Less,
        (None, None) => core::cmp::Ordering::Equal,
    }
}

fn find_entry<'a>(catalog: &'a [PkgEntry], name: &str) -> Option<&'a PkgEntry> {
    catalog.iter().find(|e| e.name == name)
}

/// `root` paketini kurmak için gereken tüm bağımlılıkları çözer.
///
/// `installed`: (ad, sürüm) çiftleri. Kurulu paketler yeniden kurulmaz; sürümü
/// kuralı sağlamıyorsa `VersionMismatch` döner (sessizce yükseltme yapılmaz).
pub fn resolve(
    catalog: &[PkgEntry],
    installed: &[(String, String)],
    root: &str,
) -> Result<InstallPlan, ResolveError> {
    if find_entry(catalog, root).is_none() {
        return Err(ResolveError::UnknownPackage(root.to_string()));
    }

    let mut plan = InstallPlan {
        to_install: Vec::new(),
        already_installed: Vec::new(),
    };
    let mut chosen: Vec<(String, String)> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    visit(
        catalog,
        installed,
        root,
        None,
        &mut plan,
        &mut chosen,
        &mut stack,
    )?;
    Ok(plan)
}

fn installed_version<'a>(installed: &'a [(String, String)], name: &str) -> Option<&'a str> {
    installed
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

/// Kurulu paket listesini VFS yollarından (`/bin/<ad>.elf`, `/bin/<ad>.pkg`)
/// (ad, sürüm) çiftlerine çevir. Sürüm `.pkg` manifestinin 2. satırından
/// okunur; ikili (`.elf`) dosyalar için sürüm boş bırakılır ve herhangi bir
/// sürüm kuralını karşılamaz.
pub fn installed_from_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for path in paths {
        let file = path.rsplit('/').next().unwrap_or(path);
        let (name, version) = if let Some(n) = file.strip_suffix(".elf") {
            (n, String::new())
        } else if let Some(n) = file.strip_suffix(".pkg") {
            (n, String::new())
        } else {
            (file, String::new())
        };
        if name.is_empty() {
            continue;
        }
        if !out.iter().any(|(n, _)| n == name) {
            out.push((name.to_string(), version));
        }
    }
    out
}

fn visit(
    catalog: &[PkgEntry],
    installed: &[(String, String)],
    name: &str,
    constraint: Option<&Dependency>,
    plan: &mut InstallPlan,
    chosen: &mut Vec<(String, String)>,
    stack: &mut Vec<String>,
) -> Result<(), ResolveError> {
    if stack.iter().any(|s| s == name) {
        let mut chain = stack.clone();
        chain.push(name.to_string());
        return Err(ResolveError::Cycle {
            chain: chain.join(" -> "),
        });
    }

    // Kurulu paket yeniden kurulmaz; sürümü kuralı sağlıyorsa atlanır,
    // sağlamıyorsa sessizce yükseltmek yerine açıkca hata verilir.
    if let Some(v) = installed_version(installed, name) {
        if let Some(req) = constraint {
            // Sürümü bilinen paket kuralı sağlamıyorsa hata; sürümü boşsa (.elf) kurulu varsay
            if !v.is_empty() && !req.matches(v) {
                return Err(ResolveError::VersionMismatch {
                    package: name.to_string(),
                    installed: v.to_string(),
                    required: req.describe(),
                });
            }
        }
        let text = if v.is_empty() {
            name.to_string()
        } else {
            format!("{} {}", name, v)
        };
        if !plan.already_installed.contains(&text) {
            plan.already_installed.push(text);
        }
        return Ok(());
    }

    if let Some((_, prev)) = chosen.iter().find(|(n, _)| n == name) {
        if let Some(req) = constraint {
            if !req.matches(prev) {
                return Err(ResolveError::Conflict {
                    package: name.to_string(),
                    wanted: prev.clone(),
                    also_wanted: req.describe(),
                });
            }
        }
        return Ok(());
    }

    // Sürüm seçimi: kuralı sağlayan en yüksek sürüm; kural yoksa en yüksek sürüm.
    let Some(entry) = catalog
        .iter()
        .filter(|e| e.name == name)
        .filter(|e| constraint.map_or(true, |req| req.matches(&e.version)))
        .max_by(|a, b| compare_version(&a.version, &b.version))
    else {
        return Err(missing_error(catalog, name, constraint));
    };

    stack.push(name.to_string());
    for dep in &entry.deps {
        match visit(
            catalog,
            installed,
            &dep.name,
            Some(dep),
            plan,
            chosen,
            stack,
        ) {
            Ok(()) => {}
            Err(missing @ ResolveError::MissingDependency { .. }) => {
                stack.pop();
                return Err(match missing {
                    ResolveError::MissingDependency { missing, .. } => {
                        ResolveError::MissingDependency {
                            package: entry.name.clone(),
                            missing,
                        }
                    }
                    _ => unreachable!(),
                });
            }
            Err(other) => {
                stack.pop();
                return Err(other);
            }
        }
    }
    stack.pop();

    chosen.push((entry.name.clone(), entry.version.clone()));
    plan.to_install
        .push(format!("{} {}", entry.name, entry.version));
    Ok(())
}

/// Uygun sürüm bulunamadığında en açıklayıcı hatayı üretir.
fn missing_error(
    catalog: &[PkgEntry],
    name: &str,
    constraint: Option<&Dependency>,
) -> ResolveError {
    if catalog.iter().any(|e| e.name == name) {
        ResolveError::UnsatisfiedVersion {
            package: name.to_string(),
            required: constraint.map(|d| d.describe()).unwrap_or_default(),
        }
    } else {
        ResolveError::MissingDependency {
            package: String::new(),
            missing: constraint
                .map(|d| d.describe())
                .unwrap_or_else(|| name.to_string()),
        }
    }
}

/// Kurulacak paketlerin kısa ad listesi (`/bin/<ad>.elf` yazımı için).
pub fn plan_names(plan: &InstallPlan) -> Vec<String> {
    plan.to_install
        .iter()
        .map(|line| line.split(' ').next().unwrap_or("").to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Vec<PkgEntry> {
        alloc::vec![
            PkgEntry::new("browser", "2.0.0").with_deps(&["tls>=1.2", "ui"]),
            PkgEntry::new("tls", "1.3.0").with_deps(&["crypto>=1.0"]),
            PkgEntry::new("tls", "1.1.0").with_deps(&["crypto>=1.0"]),
            PkgEntry::new("crypto", "1.0.4"),
            PkgEntry::new("ui", "0.9.0"),
            PkgEntry::new("orphan", "1.0.0").with_deps(&["ghost>=1.0"]),
            PkgEntry::new("loop-a", "1.0.0").with_deps(&["loop-b"]),
            PkgEntry::new("loop-b", "1.0.0").with_deps(&["loop-a"]),
            PkgEntry::new("strict", "1.0.0").with_deps(&["crypto>=2.0"]),
        ]
    }

    #[test]
    fn version_parsing_pads_missing_parts() {
        assert_eq!(parse_version("1"), Some([1, 0, 0]));
        assert_eq!(parse_version("1.2"), Some([1, 2, 0]));
        assert_eq!(parse_version("1.2.3"), Some([1, 2, 3]));
        assert_eq!(parse_version("1.2.3-rc1"), Some([1, 2, 3]));
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("beta"), None);
        assert!(compare_version("1.10", "1.9") == core::cmp::Ordering::Greater);
    }

    #[test]
    fn dependency_rules_match() {
        let d = Dependency::parse("tls>=1.2").unwrap();
        assert!(d.matches("1.2.0"));
        assert!(d.matches("1.3"));
        assert!(!d.matches("1.1.9"));
        let lt = Dependency::parse("ui < 1.0").unwrap();
        assert!(lt.matches("0.9.0"));
        assert!(!lt.matches("1.0.0"));
        let exact = Dependency::parse("crypto==1.0.4").unwrap();
        assert!(exact.matches("1.0.4"));
        assert!(!exact.matches("1.0.5"));
        // Operatörsüz kural "kurulu olsun yeter" olarak çözümlenir.
        let any = Dependency::parse("ui").unwrap();
        assert!(any.matches("0.0.1"));
        assert!(Dependency::parse("=>1.0").is_none());
    }

    #[test]
    fn resolves_transitive_dependencies_in_order() {
        let cat = catalog();
        let plan = resolve(&cat, &[], "browser").unwrap();
        assert_eq!(
            plan.to_install,
            alloc::vec![
                "crypto 1.0.4".to_string(),
                "tls 1.3.0".to_string(),
                "ui 0.9.0".to_string(),
                "browser 2.0.0".to_string(),
            ]
        );
        assert!(plan.already_installed.is_empty());
        // En yüksek uygun sürüm seçilir (tls 1.3.0, 1.1.0 değil).
        assert_eq!(
            plan_names(&plan),
            alloc::vec!["crypto", "tls", "ui", "browser"]
        );
    }

    #[test]
    fn installed_satisfying_dependency_is_skipped() {
        let cat = catalog();
        let installed = alloc::vec![("crypto".to_string(), "1.0.9".to_string())];
        let plan = resolve(&cat, &installed, "tls").unwrap();
        assert_eq!(plan.to_install, alloc::vec!["tls 1.3.0".to_string()]);
        assert_eq!(
            plan.already_installed,
            alloc::vec!["crypto 1.0.9".to_string()]
        );
        // İkili ikile göre çözümler sürümü bilmediği için kuralı sağlamayan
        // kurulu bir paketi "kuralı sağlıyor" varsayarak atlar.
        let binaries = installed_from_paths(["/bin/crypto.elf"]);
        assert_eq!(binaries, alloc::vec![("crypto".to_string(), String::new())]);
        let plan_b = resolve(&cat, &binaries, "tls").unwrap();
        assert_eq!(plan_b.to_install, alloc::vec!["tls 1.3.0".to_string()]);
        // Zaten kurulu paket tekrar listelenmez.
        let plan2 = resolve(&cat, &installed, "crypto").unwrap();
        assert!(plan2.to_install.is_empty());
    }

    #[test]
    fn missing_dependency_is_reported() {
        let cat = catalog();
        match resolve(&cat, &[], "orphan") {
            Err(ResolveError::MissingDependency { package, missing }) => {
                assert_eq!(package, "orphan");
                assert_eq!(missing, "ghost>=1.0");
                assert_eq!(
                    ResolveError::MissingDependency { package, missing }.message(),
                    "orphan paketinin bağımlılığı katalogda yok: ghost>=1.0"
                );
            }
            other => panic!("beklenmeyen sonuc: {:?}", other),
        }
    }

    #[test]
    fn installed_version_that_does_not_match_is_reported() {
        let cat = catalog();
        let installed = alloc::vec![("crypto".to_string(), "0.9.0".to_string())];
        match resolve(&cat, &installed, "tls") {
            Err(ResolveError::VersionMismatch {
                package,
                installed,
                required,
            }) => {
                assert_eq!(package, "crypto");
                assert_eq!(installed, "0.9.0");
                assert_eq!(required, "crypto>=1.0");
            }
            other => panic!("beklenmeyen sonuc: {:?}", other),
        }
    }

    #[test]
    fn unsatisfiable_catalog_version_is_reported() {
        let cat = catalog();
        match resolve(&cat, &[], "strict") {
            Err(ResolveError::UnsatisfiedVersion { package, required }) => {
                assert_eq!(package, "crypto");
                assert_eq!(required, "crypto>=2.0");
            }
            other => panic!("beklenmeyen sonuc: {:?}", other),
        }
    }

    #[test]
    fn version_range_picks_newest_satisfying_solution() {
        let cat = alloc::vec![
            PkgEntry::new("app", "1.0.0").with_deps(&["lib<2.0"]),
            PkgEntry::new("lib", "1.9.0"),
            PkgEntry::new("lib", "2.1.0"),
        ];
        let plan = resolve(&cat, &[], "app").unwrap();
        assert_eq!(
            plan.to_install,
            alloc::vec!["lib 1.9.0".to_string(), "app 1.0.0".to_string()]
        );
    }

    #[test]
    fn conflicting_constraints_are_reported() {
        let cat = alloc::vec![
            PkgEntry::new("app", "1.0.0").with_deps(&["old-lib<2.0", "new-lib"]),
            PkgEntry::new("old-lib", "1.0.0").with_deps(&["shared>=1.0"]),
            PkgEntry::new("new-lib", "1.0.0").with_deps(&["shared>=2.0"]),
            PkgEntry::new("shared", "1.0.0"),
            PkgEntry::new("shared", "2.0.0"),
        ];
        // shared>=1.0 → 2.0.0 seçilir, shared>=2.0 da 2.0.0 ile uyumludur.
        let plan = resolve(&cat, &[], "app").unwrap();
        assert!(plan.to_install.contains(&"shared 2.0.0".to_string()));

        let conflict_cat = alloc::vec![
            PkgEntry::new("app", "1.0.0").with_deps(&["old-lib", "new-lib"]),
            PkgEntry::new("old-lib", "1.0.0").with_deps(&["shared=1.0.0"]),
            PkgEntry::new("new-lib", "1.0.0").with_deps(&["shared>=2.0"]),
            PkgEntry::new("shared", "1.0.0"),
            PkgEntry::new("shared", "2.0.0"),
        ];
        match resolve(&conflict_cat, &[], "app") {
            Err(ResolveError::Conflict { package, .. }) => assert_eq!(package, "shared"),
            other => panic!("beklenmeyen sonuc: {:?}", other),
        }
    }

    #[test]
    fn unknown_package_is_reported() {
        let cat = catalog();
        assert_eq!(
            resolve(&cat, &[], "yok"),
            Err(ResolveError::UnknownPackage("yok".to_string()))
        );
    }

    #[test]
    fn dependency_cycles_terminate_with_an_error() {
        let cat = catalog();
        match resolve(&cat, &[], "loop-a") {
            Err(ResolveError::Cycle { chain }) => {
                assert!(chain.contains("loop-a"));
                assert!(chain.contains("loop-b"));
            }
            other => panic!("beklenmeyen sonuc: {:?}", other),
        }
    }
}
