//! Disk silme/taşıma işlemleri için ortak onay katmanı.
//!
//! Dosya Yöneticisi, masaüstü ve Kurulum Sihirbazı aynı kurallarla onay
//! ister:
//!
//! * **Silme** her zaman onay ister; çöp kutusundaki kalıcı silme daha
//!   yüksek risk seviyesine sahiptir.
//! * **Taşıma**(yapıştırma/kes) da onay ister — hedefte aynı isim varsa
//!   üzerine yazma onayı istenir.
//! * Görünen onay metni TR/EN olarak buradan üretilir; uygulamalar yalnızca
//!   `ConfirmKind` → dize dönüşümünü kullanır.
//! * İşlem sürerken iptal edilebilir (`OperationCancel`); iptal edilen
//!   işlem kısmi sonuç bildirir ve yarım bırakılan dosya adlarını döndürür.
//!
//! Modül saf mantıktır (`no_std` + `alloc`), host testlerinde çalışır.

/// Onay isteyen işlem türü.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConfirmKind {
    /// Çöp kutusuna taşıma (geri alınabilir).
    Delete,
    /// Kalıcı silme (geri alınamaz).
    DeletePermanent,
    /// Dosya/klasör taşıma (yapıştırma veya kes-yapıştır).
    Move,
    /// Hedefte aynı isim var; üzerine yazma.
    Overwrite,
    /// Diski biçimlendirme (tüm veriler silinir).
    FormatDisk,
}

impl ConfirmKind {
    /// Kullanıcıya gösterilen başlık (TR/EN).
    pub fn title(self, english: bool) -> &'static str {
        match (self, english) {
            (ConfirmKind::Delete, false) => "Silme Onayı",
            (ConfirmKind::Delete, true) => "Confirm Delete",
            (ConfirmKind::DeletePermanent, false) => "Kalıcı Silme Onayı",
            (ConfirmKind::DeletePermanent, true) => "Confirm Permanent Delete",
            (ConfirmKind::Move, false) => "Taşıma Onayı",
            (ConfirmKind::Move, true) => "Confirm Move",
            (ConfirmKind::Overwrite, false) => "Üzerine Yazma Onayı",
            (ConfirmKind::Overwrite, true) => "Confirm Overwrite",
            (ConfirmKind::FormatDisk, false) => "Biçimlendirme Onayı",
            (ConfirmKind::FormatDisk, true) => "Confirm Format",
        }
    }

    /// Açıklama metni (TR/EN).
    pub fn message(self, english: bool) -> &'static str {
        match (self, english) {
            (ConfirmKind::Delete, false) => {
                "Seçilen öğeler geri dönüşüm kutusuna taşınacak. Devam edilsin mi?"
            }
            (ConfirmKind::Delete, true) => {
                "Selected items will be moved to the recycle bin. Continue?"
            }
            (ConfirmKind::DeletePermanent, false) => {
                "Seçilen öğeler kalıcı olarak silinecek. Bu işlem geri alınamaz!"
            }
            (ConfirmKind::DeletePermanent, true) => {
                "Selected items will be deleted permanently. This cannot be undone!"
            }
            (ConfirmKind::Move, false) => {
                "Seçilen öğeler hedef klasöre taşınacak. Devam edilsin mi?"
            }
            (ConfirmKind::Move, true) => {
                "Selected items will be moved to the target folder. Continue?"
            }
            (ConfirmKind::Overwrite, false) => {
                "Hedefte aynı isimde bir öğe var. Üzerine yazılsın mı?"
            }
            (ConfirmKind::Overwrite, true) => {
                "An item with the same name exists at the target. Overwrite?"
            }
            (ConfirmKind::FormatDisk, false) => {
                "Hedef diskteki tüm veriler silinecek ve disk yeniden biçimlendirilecek."
            }
            (ConfirmKind::FormatDisk, true) => {
                "All data on the target disk will be erased and reformatted."
            }
        }
    }

    /// Risk seviyesi: onay penceresinin rengi ve varsayılan odak.
    pub fn risk(self) -> Risk {
        match self {
            ConfirmKind::Delete | ConfirmKind::Move => Risk::Medium,
            ConfirmKind::Overwrite | ConfirmKind::DeletePermanent | ConfirmKind::FormatDisk => {
                Risk::High
            }
        }
    }

    /// Onay diyaloğunun varsayılan odağı: yüksek riskli işlemlerde "Hayır"
    /// seçili gelir (yanlışlıkla Enter ile veri silinmesini önler).
    pub fn default_focus_yes(self) -> bool {
        !matches!(self.risk(), Risk::High)
    }
}

/// İşlem risk sınıfı.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Risk {
    Medium,
    High,
}

/// Onay akışı sonucu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Confirmation {
    Pending,
    Allowed,
    Denied,
}

/// Tek bir işlem için onay akışı.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConfirmRequest {
    pub kind: ConfirmKind,
    /// Yüksek riskli işlemlerde yazılması gereken onay kelimesi (yoksa 0).
    pub typed_required: bool,
    pub focus_yes: bool,
    pub result: Confirmation,
}

impl ConfirmRequest {
    pub fn new(kind: ConfirmKind) -> Self {
        Self {
            kind,
            typed_required: kind == ConfirmKind::FormatDisk,
            focus_yes: kind.default_focus_yes(),
            result: Confirmation::Pending,
        }
    }

    /// Tab / Shift+Tab ile Evet/Hayır arasında geçiş.
    pub fn toggle(&mut self) {
        if self.result == Confirmation::Pending {
            self.focus_yes = !self.focus_yes;
        }
    }

    /// Enter: odaklı seçeneği uygular. `typed_ok` yazılı onayın tamamlandığını
    /// bildirir (gerekli değilse yok sayılır).
    pub fn enter(&mut self, typed_ok: bool) {
        if self.result != Confirmation::Pending {
            return;
        }
        if self.focus_yes {
            if self.typed_required && !typed_ok {
                return;
            }
            self.result = Confirmation::Allowed;
        } else {
            self.result = Confirmation::Denied;
        }
    }

    /// Esc: her zaman iptal eder (güvenli varsayılan).
    pub fn escape(&mut self) {
        self.result = Confirmation::Denied;
    }

    pub fn is_allowed(self) -> bool {
        self.result == Confirmation::Allowed
    }
}

/// Uzun süren disk işlemlerinde iptal desteği.
///
/// İşlem, her adımda `is_cancelled()` çağırır; iptal edilirse kısmi
/// sonuç döndürülür ve tamamlanan/bekleyen öğe sayıları raporlanır.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct OperationCancel {
    cancelled: bool,
    completed: usize,
    total: usize,
}

impl OperationCancel {
    pub fn new(total: usize) -> Self {
        Self {
            cancelled: false,
            completed: 0,
            total,
        }
    }

    /// İptal isteği (Esc veya "İptal" düğmesi).
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Bir öğeyi tamamlandı olarak işaretler.
    pub fn step_done(&mut self) {
        if self.completed < self.total {
            self.completed += 1;
        }
    }

    /// İşlem tamamlandı mı (iptal edilmedi ve tüm öğeler bitti)?
    pub fn finished(&self) -> bool {
        !self.cancelled && self.completed >= self.total
    }

    /// Yüzde (0..=100); iptal edilmişse son yüzde korunur.
    pub fn percent(&self) -> usize {
        if self.total == 0 {
            100
        } else {
            self.completed
                .saturating_mul(100)
                .checked_div(self.total)
                .unwrap_or(0)
        }
    }

    /// İşlem sonucu (TR/EN): tamamlandı veya iptal edildi.
    pub fn summary(&self, english: bool) -> &'static str {
        match (self.cancelled, english) {
            (false, false) => "İşlem tamamlandı.",
            (false, true) => "Operation completed.",
            (true, false) => "İşlem iptal edildi (kısmi sonuç).",
            (true, true) => "Operation cancelled (partial result).",
        }
    }
}

/// Uzun işlemlerin ilerlemesini adım adım raporlar.
///
/// `OperationCancel` iptal + sayaç tutarken bu tür, kullanıcıya **gerçek**
/// bir yüzde göstermek için adım ilerlemesini tek yerden üretir. İlerleme
/// monotoniktir (asla geri gitmez) ve 100'e sabitlenir; böylece ilerleme
/// çubuğu titremez.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgressTracker {
    total: usize,
    completed: usize,
}

impl ProgressTracker {
    pub const fn new(total: usize) -> Self {
        Self {
            total,
            completed: 0,
        }
    }

    /// Bir adımı tamamlandı sayar ve güncel yüzdeyi döndürür.
    pub fn step(&mut self) -> usize {
        if self.completed < self.total {
            self.completed += 1;
        }
        self.percent()
    }

    /// Belirli bir adım sayısını tamamlandı sayar (toplu ilerleme).
    pub fn advance(&mut self, n: usize) {
        self.completed = self.completed.saturating_add(n).min(self.total);
    }

    pub fn completed(&self) -> usize {
        self.completed
    }

    pub fn total(&self) -> usize {
        self.total
    }

    /// 0..=100 aralığında tam sayı yüzde.
    pub fn percent(&self) -> usize {
        self.completed
            .saturating_mul(100)
            .checked_div(self.total)
            .unwrap_or(100)
    }

    /// İşlem bitti mi?
    pub fn is_complete(&self) -> bool {
        self.completed >= self.total
    }
}

/// İptal edilebilir işlem listesini adım adım yürütür.
///
/// `op` her öğe için `true` (başarılı) / `false` (atlandı) döndürür.
/// İşlem `progress` ile adım sonunda bildirilir; `progress` `false`
/// döndürürse işlem iptal edilir (kullanıcı "İptal"e bastı).
pub fn run_cancellable<T, Op, Progress>(
    items: &[T],
    cancel: &mut OperationCancel,
    mut op: Op,
    mut progress: Progress,
) -> usize
where
    Op: FnMut(&T) -> bool,
    Progress: FnMut(usize, usize) -> bool,
{
    let mut done = 0usize;
    for (i, item) in items.iter().enumerate() {
        if cancel.is_cancelled() {
            break;
        }
        if op(item) {
            done += 1;
        }
        cancel.step_done();
        if !progress(i + 1, items.len()) {
            cancel.cancel();
            break;
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_and_move_require_confirmation() {
        for kind in [
            ConfirmKind::Delete,
            ConfirmKind::DeletePermanent,
            ConfirmKind::Move,
            ConfirmKind::Overwrite,
            ConfirmKind::FormatDisk,
        ] {
            let req = ConfirmRequest::new(kind);
            assert!(!req.is_allowed(), "{kind:?} onay olmadan uygulanmamalı");
            assert!(!req.kind.title(false).is_empty());
            assert_ne!(req.kind.title(false), req.kind.title(true));
            assert!(!req.kind.message(false).is_empty());
            assert_ne!(req.kind.message(false), req.kind.message(true));
        }
    }

    #[test]
    fn high_risk_operations_default_to_no() {
        assert!(!ConfirmKind::DeletePermanent.default_focus_yes());
        assert!(!ConfirmKind::FormatDisk.default_focus_yes());
        assert!(!ConfirmKind::Overwrite.default_focus_yes());
        assert!(ConfirmKind::Delete.default_focus_yes());
        assert!(ConfirmKind::Move.default_focus_yes());
        assert_eq!(ConfirmKind::FormatDisk.risk(), Risk::High);
    }

    #[test]
    fn enter_and_escape_follow_keyboard_contract() {
        let mut req = ConfirmRequest::new(ConfirmKind::Delete);
        req.toggle();
        assert!(!req.focus_yes);
        req.enter(false);
        assert_eq!(req.result, Confirmation::Denied);

        let mut req = ConfirmRequest::new(ConfirmKind::Delete);
        req.enter(false);
        assert!(req.is_allowed());

        let mut req = ConfirmRequest::new(ConfirmKind::Delete);
        req.escape();
        assert_eq!(req.result, Confirmation::Denied);

        // Yazılı onay gereken işlem: kelime olmadan Enter onaylamaz.
        let mut req = ConfirmRequest::new(ConfirmKind::FormatDisk);
        req.focus_yes = true;
        req.enter(false);
        assert_eq!(req.result, Confirmation::Pending);
        req.enter(true);
        assert!(req.is_allowed());
    }

    #[test]
    fn progress_tracker_reports_real_percentages() {
        let mut p = ProgressTracker::new(4);
        assert_eq!(p.percent(), 0);
        assert!(!p.is_complete());
        assert_eq!(p.step(), 25);
        assert_eq!(p.step(), 50);
        assert_eq!(p.completed(), 2);
        assert_eq!(p.total(), 4);
        p.advance(1);
        assert_eq!(p.percent(), 75);
        assert_eq!(p.step(), 100);
        assert!(p.is_complete());
        // Fazla adımlar 100'ü aşmaz.
        p.advance(10);
        assert_eq!(p.percent(), 100);
        assert_eq!(p.completed(), 4);
    }

    #[test]
    fn progress_tracker_handles_empty_and_bulk_work() {
        // Boş iş: tamamlanmış sayılır (bant %100 ile kapanır).
        let empty = ProgressTracker::new(0);
        assert_eq!(empty.percent(), 100);
        assert!(empty.is_complete());

        // Toplu ilerleme: kopyalanan bayt gibi tek bir sayaçla.
        let mut bulk = ProgressTracker::new(1000);
        bulk.advance(250);
        assert_eq!(bulk.percent(), 25);
        bulk.advance(500);
        assert_eq!(bulk.percent(), 75);
        assert!(!bulk.is_complete());
        bulk.advance(1000);
        assert_eq!(bulk.completed(), 1000);
        assert!(bulk.is_complete());
    }

    #[test]
    fn cancelled_operation_reports_partial_result() {
        let items = [1, 2, 3, 4, 5];
        let mut cancel = OperationCancel::new(items.len());
        let mut seen = alloc::vec::Vec::new();
        let done = run_cancellable(
            &items,
            &mut cancel,
            |v| {
                seen.push(*v);
                true
            },
            |i, _| i < 3,
        );
        assert_eq!(done, 3);
        assert_eq!(seen, alloc::vec![1, 2, 3]);
        assert!(cancel.is_cancelled());
        assert!(!cancel.finished());
        assert_eq!(cancel.percent(), 60);
        assert_eq!(cancel.summary(false), "İşlem iptal edildi (kısmi sonuç).");
        assert_eq!(
            cancel.summary(true),
            "Operation cancelled (partial result)."
        );
    }

    #[test]
    fn completed_operation_reports_full_progress() {
        let items = [1u8, 2, 3];
        let mut cancel = OperationCancel::new(items.len());
        let done = run_cancellable(&items, &mut cancel, |_| true, |_, _| true);
        assert_eq!(done, 3);
        assert!(cancel.finished());
        assert_eq!(cancel.percent(), 100);
        assert_eq!(cancel.summary(false), "İşlem tamamlandı.");
        // Boş liste: tamamlanmış sayılır.
        let empty: [u8; 0] = [];
        let mut c0 = OperationCancel::new(0);
        assert_eq!(run_cancellable(&empty, &mut c0, |_| true, |_, _| true), 0);
        assert_eq!(c0.percent(), 100);
    }

    #[test]
    fn step_done_is_bounded() {
        let mut c = OperationCancel::new(2);
        for _ in 0..5 {
            c.step_done();
        }
        assert_eq!(c.percent(), 100);
    }
}
