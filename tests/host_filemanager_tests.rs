//! Dosya Yöneticisi klavye navigasyonu ve ilerleme çubuğu host testleri.
//!
//! `gui::toast` benzeri, `filemanager` modülü `Framebuffer`/VFS'ye bağımlı
//! olduğu için host'ta tam modül derlenmez. Bu testler, `src/apps/filemanager.rs`'deki
//! **saf** mantığı bağımsız olarak kodar:
//! - `SelectMod::from_keys`: Shift/Ctrl modifier eşlemesi
//! - `OpProgress`: ilerleme yüzdesinin monotonluğu, set_steps/set_percent davranışı

// --- SelectMod: Shift/Ctrl modifier eşlemesi (filemanager.rs ile aynı sözleşme) ---

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SelectMod {
    None,
    Shift,
    Ctrl,
    CtrlShift,
}

impl SelectMod {
    fn from_keys(shift: bool, ctrl: bool) -> Self {
        match (shift, ctrl) {
            (true, true) => SelectMod::CtrlShift,
            (true, false) => SelectMod::Shift,
            (false, true) => SelectMod::Ctrl,
            (false, false) => SelectMod::None,
        }
    }

    /// Shift'li gezinmede seçim çapası (anchor) yerinde kalır mı?
    fn keeps_anchor(self) -> bool {
        matches!(self, SelectMod::Shift | SelectMod::CtrlShift)
    }
}

#[test]
fn selectmod_from_keys_maps_modifiers() {
    assert_eq!(SelectMod::from_keys(false, false), SelectMod::None);
    assert_eq!(SelectMod::from_keys(true, false), SelectMod::Shift);
    assert_eq!(SelectMod::from_keys(false, true), SelectMod::Ctrl);
    assert_eq!(SelectMod::from_keys(true, true), SelectMod::CtrlShift);
}

#[test]
fn selectmod_keeps_anchor_for_shift_variants() {
    assert!(!SelectMod::None.keeps_anchor());
    assert!(!SelectMod::Ctrl.keeps_anchor());
    assert!(SelectMod::Shift.keeps_anchor());
    assert!(SelectMod::CtrlShift.keeps_anchor());
}

// --- OpProgress: ilerleme çubuğu durumu (filemanager.rs ile aynı sözleşme) ---

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct OpProgress {
    label: (&'static str, &'static str),
    percent: u8,
    total: usize,
    done: usize,
}

impl OpProgress {
    const fn new(label: (&'static str, &'static str), total: usize) -> Self {
        Self {
            label,
            percent: 0,
            total,
            done: 0,
        }
    }

    /// Adım ilerlemesini uygular; yüzde monoton olarak artar.
    fn set_steps(&mut self, done: usize, total: usize) {
        self.done = done.min(total.max(done));
        self.total = total;
        let pct = done
            .saturating_mul(100)
            .checked_div(total)
            .map_or(100, |p| p.min(100)) as u8;
        if pct > self.percent {
            self.percent = pct;
        }
    }

    /// Yüzdeyi doğrudan ayarlar (1..=99 beklenir; aşama geçişlerinde kullanılır).
    fn set_percent(&mut self, pct: u8) {
        let pct = pct.min(100);
        if pct > self.percent {
            self.percent = pct;
        }
    }

    /// Görünüm diline göre etiketi döndürür.
    fn label_str(&self, english: bool) -> &'static str {
        if english {
            self.label.1
        } else {
            self.label.0
        }
    }
}

#[test]
fn op_progress_starts_at_zero() {
    let p = OpProgress::new(("Kopyalanıyor", "Copying"), 10);
    assert_eq!(p.percent, 0);
    assert_eq!(p.done, 0);
    assert_eq!(p.total, 10);
    assert_eq!(p.label_str(true), "Copying");
    assert_eq!(p.label_str(false), "Kopyalanıyor");
}

#[test]
fn op_progress_percent_is_monotonic() {
    let mut p = OpProgress::new(("Siliniyor", "Deleting"), 8);
    p.set_steps(2, 8);
    assert_eq!(p.percent, 25);
    p.set_steps(4, 8);
    assert_eq!(p.percent, 50);
    // Geriye doğru adım yüzdesini düşürmemeli.
    p.set_steps(1, 8);
    assert_eq!(p.percent, 50);
    // Tamamlandığında %100.
    p.set_steps(8, 8);
    assert_eq!(p.percent, 100);
    assert_eq!(p.done, 8);
}

#[test]
fn op_progress_set_percent_is_monotonic() {
    let mut p = OpProgress::new(("x", "x"), 0);
    p.set_percent(30);
    assert_eq!(p.percent, 30);
    p.set_percent(20); // düşürülmemeli
    assert_eq!(p.percent, 30);
    p.set_percent(70);
    assert_eq!(p.percent, 70);
    p.set_percent(150); // clamp'e tabi
    assert_eq!(p.percent, 100);
}

#[test]
fn op_progress_set_steps_with_zero_total() {
    let mut p = OpProgress::new(("x", "x"), 0);
    // total = 0 olduğunda checked_div None döner → %100.
    p.set_steps(0, 0);
    assert_eq!(p.percent, 100);
}
