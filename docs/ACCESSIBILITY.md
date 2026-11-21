# Accessibility

minOs, Rust ile geliştirilen bağımsız bir 64-bit (`x86_64`) işletim sistemidir. Grafik masaüstü, yerleşik uygulamalar ve bir web tarayıcısı içerir. Erişilebilirlik, minOs'un herkes tarafından kullanılabilir olmasına yönelik taahhüdünün bir parçasıdır: görme, işitsel, hareket veya kognitif farklılıkları olan kullanıcılar dahil. Bu belge, minOs'un erişilebilirlik taahhütlerini, önceliklerini, katkıda bulunanların beklediklerini ve erişim engellerinin nasıl raporlanacağını açıklar. Dildeki teknik terimler yerine kullanıcı deneyimleri üzerinden yazılmıştır.

## Priorities

minOs, aşağıdaki erişilebilirlik sonuçlarını önceliklendirir:

- **Klavyeyle gezinme:** Tüm formlar, menüler ve pencereler Tab / Shift+Tab ile gezilebilir. Odaklı denetim çevresinde renkli bir odak halkası çizilir (gui::widget::Ui ve MinGuiCompositor::draw_focus_ring). Enter varsayılan işlemi çalıştırır; Escape iletişim kutularını kapatır.
  - **Dosya Yöneticisi:** Ok tuşları imleci (ve ızgarada satır) taşır; **Enter** açar/girer, **Backspace** üst klasöre çıkar, **Delete** siler, **F2** yeniden adlandırır. Çoklu seçim için **Shift** (çapa-aralık) ve **Ctrl** (tek tek ekle/çıkar) kullanılır; Ctrl+A tümünü seçer, Ctrl+Shift+A temizler. Home/End/PageUp/PageDown da Shift/Ctrl ile aynı kuralları izler. İmleç satırı, seçili olmasa da odak halkasıyla görünür.
- **Bildirimler:** Toast bildirimleri türlerine göre renk ve ikon taşır (bilgi mavi “i”, başarı yeşil “✓”, uyarı sarı “!”, hata kırmızı “✕”). Otomatik kapanma süresi türe göredir (bilgi/başarı 3 sn, uyarı 4 sn, hata 5 sn); **Esc** açık bildirimleri kapatır, üzerine gelmek sayacı duraklatır. Aynı metin ekran okuyucu bildirim kuyruğuna da (durum/uyarı) eklenir.
- **İlerleme:** Kopyalama, silme, küçük resim ve disk formatı gibi uzun işlemler gerçek yüzde ve ilerleme çubuğu gösterir (dosya yöneticisi işlem bandı ve bildirim).
- **Renk ve kontrast:** Tüm UI renkleri tema jetonları üzerinden çözümlenir. `HighContrast` teması, gövde metni ve odak/çerçeve renkleri için pencere arka planına göre en az 7:1 kontrast oranı hedefler (host testlerinde doğrulanır). Diğer temalar için başlık/metin ve pencere arka planı/aksan renkleri arasında en az 1.8:1 görünürlük korunduğu test edilir.
- **Yazı ölçeği:** Font yakınlaştırması %100, %125 ve %150 olarak ayarlanabilir; düğme, menü ve liste boyutları bu ölçeğe orantılı büyütülür (`gui::sp`).
- **Hareketi azaltma:** Sistem Ayarları > Ekran sekmesindeki "Arayüz animasyonları" anahtarı tüm hareketi kapatır (pencere açılış, geçiş, tooltip ve toast animasyonları).
- **Dil desteği:** Tüm kullanıcılara yönelik metinler Türkçe ve İngilizce olarak yerelleştirilmiştir (`gui::ui_text`). İki dildeki metinler aynı anlamı taşımak zorundadır; host testleri TR/EN eşitsizliğini doğrular.

Bu hedefler istek üzerine değerlendirme sonrası doğrulanmış bir uygunluk iddiası değildir; hedefler, projenin erişilebilirlik yol haritasının parçasıdır.

## Contributor expectations

Katkıda bulunanlar, kullanıcıya yönelik her değişiklikle aşağıdaki erişilebilirlik koruma ötesini uygulamalıdır:

- **Klavye odak sırası:** Yeni bir form veya iletişim kutusu eklendiğinde Tab / Shift+Tab sırası kontrol edilmeli; devre dışı bırakılan denetimler odak sırasından dışlanmalı (`Widget::enabled` ve `Widget::focusable`).
- **Düğme durumları:** Düğmelerin seçili, odaklanmış ve devre dışı durumları görsel olarak ayrıştırılmalı (widget.rs'deki `Button::enabled`, `Button::selected` ve çizim mantığı).
- **Hata mesajları:** Kullanıcıya gösterilen her hata, hata metni yanında somut bir çözüm önerisi içermelidir (`apps/errors.rs`'deki `AppErrorKind::recovery`).
- **Yerelleştirme:** Her yeni kullanıcı metni Türkçe ve İngilizce olarak eklenmeli; iki versiyonun aynı anlamı taşıması gerekir.
- **Testler:** Kullanıcıya görünen davranışını etkileyen değişiklikler, `tests/host_gui_tests.rs` veya ilgili host test dosyasına eklenmeli. Tema kontrastı, odak halkası ve TR/EN yerelleştirme testleri `test_host.ps1` ile çalıştırılabilir.

## Reporting accessibility issues

Erişim engeliyle karşılaşırsanız veya yardım istiyorsanız, GitHub deposunda yeni bir issue açın:

https://github.com/tbagriyanik/minos/issues

İşe yarayan bir rapor şunları içerebilir:

- Etkilenen görev (ör. "Kontrol Paneli'nde depolama bilgisi okunamıyor", "Ayarlar formunda Tab ile gezinilemiyor")
- Gözlemlenen davranış ve beklediğiniz davranış
- İşletim sistemi (minOs sürümü), donanım ve ekran çözünürlüğü
- Kullandığınız yardımcı teknoloji (ekran okuyucu, yüksek kontrast teması vb.)

Ekran görüntüsü veya kayıt isteğe bağlıdır ve zorunlu değildir. Engelle ilgili olarak özür dilişmeniz veya disabilità açıklamanız istenmez.

### Severity

Proje, şu şiddet düzeylerini kullanır:

- **Kritik:** Kullanıcı temel bir görevi tamamlayamaz (ör. pencere kapatılamıyor, klavye odak sırası kırılmış, Escape ile iletişim kutusu kapanmıyor).
- **Yüksek:** Görev mümkün ancak çok zor (ör. odaklı denetim görsel olarak belirgin değil, kontrast çok düşük).
- **Orta:** Kullanıcı deneyimini etkileyen ancak geçici bir çözüm olan sorun (ör. Türkçe/İngilizce metinlerde küçük anlam farkı).
- **Düşük:** İyileştirme önerisi veya estetik erişilebilirlik eksikliği.

Şiddet düzeyleri bakıcılar tarafından tartışma sırasında atanır veya onaylanır; rapor veren zorunda değildir.

### How we respond

Bir erişilebilirlik raporu alındığında şunları taahhüt ederiz:

- 5 iş günü içinde kabul ve ilk yanıt
- Durum güncellemeleri ve geçici çözüm önerileri (varsa)
- Düzeltilmesi planlanan değişiklikler için doğrulama fırsatı (test veya QEMU üzerinden)

Bu taahhütler, proje bakıcılarının sürekli yerine getirebileceği kapsamda tutulur.

## Ownership and maintenance

Erişilebilirlik sahibi minOs proje bakıcısıdır (https://github.com/tbagriyanik/minos). Sorumlulukları şunlardır:

- Erişilebilirlik tartışmalarının ve issue'ların incelenmesi
- Yeni temalar ve UI değişikliklerinin erişilebilirlik koruma ötesine uygunluğu
- Bu belgenin düzenli olarak güncellenmesi

Sorumluluk devredildiğinde, yeni sahibin bu belge ve ilgili issue'lar üzerinden bilgilendirilmesi sağlanır.

## Supported environments

minOs şu ortamlarda desteklenmiş veya test edilmiştir:

- **Platform:** x86_64 (QEMU, gerçek donanım)
- **Giriş yöntemleri:** Klavye (PS/2 ve USB HID), fare (USB HID), MouseKeys (sayısal tuş takımıyla fare sürme)
- **Temalar:** VelvetDark (varsayılan), Daylight, Nord, TerminalGreen, Sunset, Dracula, Monokai, SolarizedDark, HighContrast
- **Yazı ölçeği:** %100, %125, %150
- **Diller:** Türkçe ve İngilizce

Kısmi destek: Ekran okuyucu uyumluluğu henüz tam olarak değerlendirilmemiştir. Odak halkası ve kontrast gibi görsel erişilebilirlik özellikleri varsayılan temada mevcuttur.

## Known limitations

Aşağıdaki erişim engelleri bilinmektedir:

- **Ekran okuyucu desteği:** minOs, bir işletim sistemi çekirdeği ve grafik masaüstü olduğu için geleneksel ekran okuyucu API'sine (ör. ARIA, MSAA) sahip değildir. Odak halkası ve yüksek kontrast teması, kısmi bir eşdeğer erişim sağlar.
- **Küçük ekranlar:** 240x160 gibi çok düşük çözünürlüklerde bazı pencereler ve metinler ekran dışına çıkabilir; `screen_layout` modülünde kapanış ve açılış kutuları sınırlı çözünürlüklerde sınanmıştır.
- **İşaret dili / sesli komut:** Henüz desteklenmemektedir.

Bu sınırlamalar için henüz izleme issue'ı açılmamışsa, lütfen yukarıdaki raporlama sürecini kullanın.

## Feedback and improvements

Bu belge veya minOs erişilebilirlik uygulamalarıyla ilgili önerileriniz için GitHub deposunda issue açabilirsiniz. Aktif erişim engelleri doğrudan yukarıdaki "Reporting accessibility issues" bölümünde açıklanan süreçten iletilmelidir.
