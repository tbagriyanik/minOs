# minOs Sistem Çağrısı ABI Referansı

> Durum: **etkin** · Uygulama: `src/mm/process.rs` (`syscall_dispatch_abi`), giriş noktası: `src/arch/x86_64/ring3.rs` (`int $0x80`).
> Bu belge tek doğruluk kaynağıdır; bir çağrının numarası, argümanları veya dönüş
> semantiği değiştiğinde burada güncellenmelidir.

## 1. Giriş Mekanizması

| Öğe | Değer |
| :--- | :--- |
| Komut | `int $0x80` (`CD 80`) |
| Kapı | IDT vektör `0x80`, **DPL = 3** (Ring 3'ten çağrılabilir) |
| Giriş adresi | `SYSENTER`/`SYSCALL` **yok**; yazılım kesmesi kullanılır |
| Ayrıcalık geçişi | Ring 3 → Ring 0; çekirdek yığını `TSS.rsp0` (süreç başına 4 × 4 KiB) |
| Dönüş | `iretq` (naked stub `int80_entry` → `rust_int80`) |
| Hata durumu | Dönüş değeri `u64::MAX` (`0xFFFF_FFFF_FFFF_FFFF`) |
| Kesme geçmişi | Girişte `IF` temizlenir; `iretq` kullanıcı `RFLAGS`'ini geri yükler |

### 1.1 Yazmaç Sözleşmesi (SysV uyumlu)

| Yazmaç | Rol |
| :--- | :--- |
| `RAX` | Çağrı numarası (giriş) / dönüş değeri (çıkış) |
| `RDI` | Argüman 1 |
| `RSI` | Argüman 2 |
| `RDX` | Argüman 3 |
| `R10` | Argüman 4 *(2026-11: eklendi; önceden yok sayılıyordu)* |
| `RCX`, `R11` | Çekirdek tarafından korunur (syscall/sysret uyumluluğu) |
| `RBX`, `RBP`, `R8`, `R9`, `R12–R15` | Girişte saklanır, çıkışta aynen geri yüklenir |
| `XMM0–XMM15`, MXCSR | `fxsave64`/`fxrstor64` ile korunur |

Çekirdek **hiçbir genel amaçlı yazmacı** dönüşte bozmaz; yalnızca `RAX` dönüş
değerini taşır (Ring3 register-restore testi: `[RING3] GPR ve XMM yazmaclari
syscall donusunde korundu`).

### 1.2 Kullanıcı Bellek Erişimi

Çekirdek, kullanıcı işaretçilerini **asla doğrudan dereference etmez**. Tüm
kopyalama `src/mm/uaccess.rs` üzerinden yapılır:

* Aralığın **tamamı** doğrulanmadan tek bayt bile kopyalanmaz (atomiklik);
  yarı eşlenmiş tampon kısmi veri sızdırmaz.
* Her sayfa `AddressSpace::translate_user()` ile dört sayfalama seviyesinde
  `PRESENT | USER` aranarak çözümlenir; `0x0000_8000_0000_0000` (kanonik alt
  yarı) dışındaki adresler ve `NULL` reddedilir.
* Yazma yolları ayrıca `WRITABLE` bitini ve `PAGE_COW == 0` koşulunu arar.
  COW sayfasına çekirdekten yazmak kullanıcının `#PF` yolunu atlayacağı için
  reddedilir; çağıran önce kullanıcı tarafında yazdırmalıdır.
* Tek kopyalamada azami `MAX_USER_COPY = 1 MiB`; aşan istek `MAX` döner.
* Kopyalama sayfa sayfa `copy_nonoverlapping` ile yapılır; bayt başına sayfa
  tablosu yürüyüşü yoktur.

## 2. Çağrı Tablosu

`—` = kullanılmıyor · `ptr` = kullanıcı sanal adresi · Tüm uzunluklar bayt.

| No | Ad | Arg1 (`RDI`) | Arg2 (`RSI`) | Arg3 (`RDX`) | Arg4 (`R10`) | Dönüş (`RAX`) |
| ---: | :--- | :--- | :--- | :--- | :--- | :--- |
| 0 | `SYS_YIELD` | — | — | — | — | `0` |
| 1 | `SYS_EXIT` | `code: i32` | — | — | — | `0` (dönmez) |
| 2 | `SYS_GETPID` | — | — | — | — | `pid` |
| 3 | `SYS_ALLOC_FRAME` | — | — | — | — | fiziksel çerçeve veya `0` |
| 4 | `SYS_FREE_FRAME` | `frame: u64` | — | — | — | `0` / `MAX` |
| 5 | `SYS_WRITE` | `buf: ptr` | `len: usize` | — | — | yazılan bayt / `0` |
| 6 | `SYS_READ` | `fd: usize` | `buf: ptr` | `len: usize` | — | okunan bayt / `MAX` |
| 7 | `SYS_SPAWN` | — | — | — | — | **ayrılmış**, `MAX` |
| 8 | `SYS_EXEC` | `image: ptr` | `len: usize` | — | — | `0` / `MAX` |
| 9 | `SYS_FORK` | — | — | — | — | çocuk `pid` / `MAX` |
| 10 | `SYS_WAIT` | `pid` veya `0` | `status: ptr` veya `0` | — | — | `pid` / `0` (bloke) / `MAX` |
| 11 | `SYS_BRK` | `new_brk: u64` | — | — | — | `HEAP_BASE + new_brk` / `MAX` |
| 12 | `SYS_MMAP` | `len: u64` | `flags: u64` | — | — | `addr` / `0` |
| 13 | `SYS_MUNMAP` | `addr: u64` | `len: u64` | — | — | `0` / `MAX` |
| 14 | `SYS_MEMINFO` | `out: ptr` veya `0` | — | — | — | `0` / özet / `MAX` |
| 15 | `SYS_OPEN` | `path: ptr` (NUL sonlu) | `flags: u32` | — | — | `fd` / `MAX` |
| 16 | `SYS_CLOSE` | `fd: usize` | — | — | — | `0` / `MAX` |
| 17 | `SYS_RESINFO` | `out: ptr` | — | — | — | `0` / `MAX` |
| 18 | `SYS_LSEEK` | `fd: usize` | `offset: i64` | `whence: u64` | — | yeni konum / `MAX` |
| 19 | `SYS_KILL` | `pid: usize` | `sig: u64` | — | — | `0` / `MAX` |

### 2.1 `SYS_YIELD` (0)

Süreç `Ready` durumuna alınır ve tam bağlamı (GPR + `RIP/CS/RFLAGS/RSP/SS`)
kesme çerçevesinden `ProcessContext`'e kopyalanır; aynı dilimde geri
dönülmez. `0` döner.

### 2.2 `SYS_EXIT` (1)

`code` dönüş kodu olarak kaydedilir, süreç `Terminated` işaretlenir ve
torunları PID 0'a yeniden ebeveynlenir. Bekleyen ebeveyn (`SYS_WAIT`) varsa
`wake_waiting_parent` onu uyandırır; `RDI` (exit kodu) ebeveynin `status`
işaretçisine yazılır. Kaynak iadesi sürecin kendi CR3'ü altında yapılmaz;
`reap_terminated` çekirdek CR3'ü aktifken iade eder.

### 2.3 `SYS_GETPID` (2)

`sched.current_pid` döner. PID 0 çekirdek sürecidir.

### 2.4 `SYS_ALLOC_FRAME` (3) / `SYS_FREE_FRAME` (4)

Fiziksel PMM ile çalışır; sayfa **kullanıcı adres uzayına eşlenmez**. Amaç
DMA/donanım benzeri fiziksel erişimdir. Çerçeve ayrıca PMM'in `u64` değeri
olarak döner (HHDM sanal adresi değil). `SYS_FREE_FRAME` yalnızca **çağıranın
kendi kaynak defterinde** olan çerçeveleri kabul eder (`take_user_frame`);
sahipsiz çerçeve `MAX` döner ve PMM'e dokunulmaz.

| Durum | `ALLOC_FRAME` | `FREE_FRAME` |
| :--- | :--- | :--- |
| Başarı | fiziksel adres | `0` |
| Bellek yok | `0` | — |
| Sahiplik yok | — | `MAX` |

### 2.5 `SYS_WRITE` (5) / `SYS_READ` (6)

`buf` kullanıcı adresinden `len` baytı `uaccess::copy_from_user` /
`copy_to_user` ile **sayfa sayfa** kopyalar. `SYS_WRITE` seri konsola yazar
(fd argümanı yoktur); `len == 0` veya `len > 1024` → `0`. Geçersiz
kullanıcı aralığında **hiçbir bayt** kopyalanmaz (ön doğrulama atomiktir).
`SYS_READ` için hedef sayfa yazılabilir ve `PAGE_COW` içermiyor olmalıdır;
aksi halde `MAX` döner.

### 2.6 `SYS_EXEC` (8)

`image` (`len` bayt, en fazla `MAX_EXEC_IMAGE_SIZE` = 112 KiB) kullanıcı
belleğinden kopyalanır ve **yeni bir adres uzayında** yüklenir:

* başarılıysa eski adres uzayı (çekirdek yığını hariç) iade edilir, `CR3`
  değişir, `RIP = entry`, `RSP = stack_pointer` olur ve dönüş `0`'dır;
  `int80` yolu `RIP/RSP`'i kesme çerçevesine yazar.
* yükleme başarısızsa **eski imaj korunur** ve `MAX` döner (atomik exec).

ASLR: yeni imajın mmap tabanı yeniden rastgele seçilir.

### 2.7 `SYS_FORK` (9)

**Copy-on-write** çocuk üretir (bkz. Bölüm 4). Ebeveynde `RAX` = çocuk PID,
çocukta `RAX` = `0`, `RIP/RSP` ebeveyninkiyle aynı. Hata → `MAX`.

### 2.8 `SYS_WAIT` (10)

| Argüman | Anlam |
| :--- | :--- |
| `pid` = `0` | herhangi bir alt süreç |
| `pid` ≠ `0` | yalnızca o alt süreç |
| `status` ≠ `0` | exit kodu bu adrese `i32` (LE) yazılır; adres **önceden** `translate_user` ile doğrulanır |

Dönüş semantiği:

* Alt süreç zaten `Terminated` → `pid` döner, kaynakları iade edilir, süreç
  tablosundan silinir.
* Alt süreç yaşıyor → ebeveyn `Blocked` olur, `waiting_child` ve
  `wait_status_addr` kaydedilir, dilim burada biter. Çocuk çıkınca ebeveyn
  `Ready`'ye döner, `RAX = çocuk_pid`, `status` doldurulur (bu çağrı `0`
  döndürmüş gibi görünür çünkü uyanan ebeveyne `pid` yazılır).
* Alt süreç yok → `MAX`.

### 2.9 `SYS_BRK` (11)

Kullanıcı heap'inin ucunu ayarlar. Aralık: `HEAP_BASE = 0x0100_0000` (16 MiB),
`HEAP_LIMIT = 0x0400_0000` (64 MiB).

* `new_brk > HEAP_LIMIT` veya `new_brk > limits.max_heap_bytes` → `MAX`, uç
  değişmez.
* Büyütme: `[eski_uç, yeni_uç)` sayfaları **tembel (lazy)** eşlenir —
  PTE `PRESENT | USER | NX | PAGE_LAZY` (yazılabilir değil) olarak yazılır
  ve paylaşılan **sıfır sayfası** çerçevesini gösterir. Okumalar sıfır
  döner; ilk yazma koruma ihlali `#PF` üretir ve `handle_lazy_fault`
  sıfırlanmış özel çerçeve ayırır. Böylece büyütülen ama yazılmayan heap
  fiziksel bellek harcamaz.
* Küçültme: uçtan çıkan sayfalar `unmap` edilir; gerçekleşmiş çerçeveler
  PMM'e döner.
* Başarı → `HEAP_BASE + new_brk`.

Heap başlangıcı kod segmentinden (4 MiB) uzaktır; `HEAP_BASE` öncesi
eşlenmemiş olduğundan heap'ten aşağı taşma #PF üretir.

### 2.10 `SYS_MMAP` (12) / `SYS_MUNMAP` (13)

* `SYS_MMAP(len, flags)`: `len` 4 KiB'e yukarı yuvarlanır; bölge
  `mmap_base + mmap_used` adresine eklenir. `mmap_base` süreç ömrü boyunca
  sabittir ve ASLR ile `[0x0400_0000, 0x1000_0000]` (64 MiB – 256 MiB)
  aralığında **2 MiB hizalı** seçilir. `flags` içinde `USER` yoksa `USER_DATA` kullanılır.
  Dönüş: bölge adresi; hata → `0`.
* Anonim bölgeler **tembel** eşlenir: PTE'ler paylaşılan sıfır sayfasını
  gösteren salt-okunur `PAGE_LAZY` girdileridir; okumalar sıfır döner,
  ilk yazma `#PF` → `handle_lazy_fault` ile özel çerçeve ayırır.
  `flags` bit 20 (`MAP_SHARED_FILE = 1 << 20`) set ise
  bölge dosya-paylaşımlı olur: aynı adres aralığını daha önce eşlemiş bir
  süreç varsa çerçeveler kopyalanmadan paylaşılır (zero-copy), yoksa yeni
  bir paylaşımlı çerçeve havuzu kurulur. `munmap` yalnızca referans bırakır.
* Sınırlar: `len ≤ 64 MiB`, en fazla 16 açık bölge, canlı bölge baytları
  toplamı `limits.max_mmap_bytes` (varsayılan 64 MiB) aşamaz; bölge kullanıcı
  yığınına ulaşamaz. `munmap` kotayı geri açar.
* `SYS_MUNMAP(addr, len)`: `addr`'ı **içeren** bölgeyi bulur (adres anahtarı),
  tüm sayfalarını serbest bırakır. Başarı → `0`; bilinmeyen/ikinci kez →
  `MAX`.

### 2.11 `SYS_MEMINFO` (14)

Çağıranın kullanıcı bellek özetini raporlar.

* `out == 0` → 12 alanlı raporun katlanmış özeti (`v ^= v.rotate_left(7)`
  zinciri); sanity kontrolü için.
* `out != 0` → on iki `u64` (LE, toplam 96 bayt) `out` adresine yazılır;
  dönüş `0`. Hedef yazılabilir değilse `MAX`.

Rapor alanları (ilk dördü ABI uyumluluğu için korunur):

| # | Alan |
| :- | :--- |
| 0 | `heap_bytes` — `brk` heap ucunun `HEAP_BASE` göreli boyu |
| 1 | `mmap_regions` — açık mmap bölgesi sayısı |
| 2 | `mmap_bytes` — canlı mmap baytları |
| 3 | `heap_mapped_pages` — heap için eşlenmiş sayfa sayısı |
| 4 | `user_frame_count` — kullanıcı verisi çerçeve sayısı |
| 5 | `shared_pages` — heap+mmap'te COW/paylaşımlı sayfa sayısı |
| 6 | `shared_saved_bytes` — paylaşımın tasarruf ettiği bayt |
| 7 | `total_bytes` — sistem toplam fiziksel bellek |
| 8 | `free_bytes` — sistem boş fiziksel bellek |
| 9 | `user_frames` — kullanıcı çerçeveleri (sistem geneli) |
| 10 | `kernel_frames` — çekirdek çerçeveleri |
| 11 | `heap_mapped_bytes` — kernel heap'e eşlenmiş bayt |

### 2.12 `SYS_OPEN` (15) / `SYS_CLOSE` (16)

* `SYS_OPEN(path, flags)`: `path` kullanıcı belleğinden NUL'a kadar
  (`MAX_FD_PATH = 96` bayt sınırıyla) kopyalanır. Yol **mutlak** olmalı, `..`
  ve boş bileşen içermemelidir. Yalnızca `O_RDONLY` desteklenir; yazma modu
  `MAX` döner. Başarı → en küçük boş fd (`3+`); hata → `MAX`.
* Aynı yol ikinci kez açılırsa içerik yeniden yüklenmez; çekirdek genelindeki
  `FileTable` nesnesinin referans sayacı artırılır.
* Dosya içeriği VFS açılış önbelleğinden, yerleşik tablodan (`/etc/fd-test`,
  `/dev/null`) veya FAT disk görünümünden gelir. VFS önbelleği en fazla
  2 MiB'dir. Sınırlar: nesne başına `MAX_FILE_BYTES = 256 KiB`, süreç başına
  `MAX_FDS = 16` (0/1/2 dahil), çekirdek genelinde `MAX_OPEN_FILES = 32`.
* `SYS_CLOSE(fd)`: yalnızca kullanıcı fd'lerini (`fd ≥ 3`) kabul eder; son
  referans bırakıldığında nesne içeriği serbest kalır. Başarı → `0`;
  geçersiz/kapalı fd → `MAX`. 0/1/2 kapatılamaz.

### 2.13 `SYS_READ` (6)

`SYS_READ(fd, buf, len)`: `fd`'nin mevcut imlecinden en fazla `len` bayt okur
ve `uaccess::copy_to_user` ile `buf`'a yazar. Dönüş: okunan bayt (dosya sonunda
`0`). `stdin` (fd 0) şimdilik EOF döner; `stdout`/`stderr` → `MAX`. Geçersiz
fd, yazılamaz hedef veya `len > MAX_USER_COPY` → `MAX`. İmleç yalnızca başarılı
kopyalamadan sonra ilerler.

### 2.14 `SYS_LSEEK` (18)

`SYS_LSEEK(fd, offset, whence)`: `whence` = `0` (SET), `1` (CUR), `2` (END).
Hedef `0..=dosya_boyu` aralığında olmalıdır; dışına taşma, negatif sonuç ve
desteklenmeyen `whence` → `MAX`. Başarı → yeni konum (bayt). Standart
tanıtıcılar `MAX` döner.

### 2.14b `SYS_KILL` (19)

`SYS_KILL(pid, sig)`: hedef sürece sinyal gönderir veya süreç denetimi yapar.
Numaralar Linux ile uyumludur (`src/mm/process.rs::signals`).

| `sig` | Etki | Dönüş |
| ---: | :--- | :--- |
| `0` | Yalnızca varlık denetimi (kill(2) probe) | süreç varsa `0`, yoksa `MAX` |
| `9` `SIGKILL`, `15` `SIGTERM` | Süreç `-(sig)` çıkış koduyla `Terminated` olur (kaynaklar reap edilir) | `0` / `MAX` |
| `19` `SIGSTOP` | `Ready`/`Running` süreç `Suspended` olur; bağlam korunur, zamanlayıcı seçmez | `0` / `MAX` |
| `18` `SIGCONT` | `Suspended` süreç `Ready`'ye döner; diğer durumlarda zararsız başarı | `0` / `MAX` |
| diğer `1..=64` | `pending_signals` bit maskesine işlenir (kullanıcı işleyicisi teslimi henüz yok) | `0` / `MAX` |
| `> 64` veya `pid == 0` | Geçersiz | `MAX` |

* `SIGSTOP`'u **kendisi çağıran** süreç kullanıcıya dönmez; bağlamı
  `int 0x80` dönüş noktası olarak kaydedilir ve `rax` sonuçla (`0`) yazılır.
  `SIGCONT` sonrası `kill()` çağrısı `0` döndürür.
* `Blocked` durumundaki süreç şu an duraklatılamaz (tek durum alanı
  blocked+stopped birleşimini temsil edemez); `SIGSTOP` → `MAX`.
* PID 0 (`kernel_core`) hiçbir sinyalle sonlandırılamaz/duraklatılamaz.
* `wait`/`reap` çıkış kodu negatif sinyal numarası taşır (`-9`, `-15`);
  `-11` (`SIGSEGV`) ile aynı kural.

### 2.15 `SYS_RESINFO` (17)

Çağıranın kaynak limitlerini ve anlık kullanımını `out` adresine sekiz `u64`
(LE) olarak yazar; dönüş `0`, yazılamaz hedef → `MAX`.

| İndeks | Alan |
| ---: | :--- |
| 0 | `limits.max_frames` (toplam çerçeve) |
| 1 | `frame_count` (kullanılan toplam çerçeve) |
| 2 | `limits.max_user_frames` |
| 3 | `user_frame_count` |
| 4 | `limits.max_fds` |
| 5 | kullanılan fd sayısı (0/1/2 dahil) |
| 6 | `limits.max_mmap_bytes` |
| 7 | canlı mmap baytları |

Varsayılan kullanıcı limitleri: 64 toplam çerçeve, 32 kullanıcı çerçevesi,
16 fd, 64 MiB mmap, 64 MiB heap. PID 0 (`kernel_core`) sınırsızdır; limitler
`fork` ile devralınır ve `exec` sonrası korunur.

### 2.16 fd / Limit Yaşam Döngüsü

| Olay | fd tablosu | Nesne referansları | Limitler |
| :--- | :--- | :--- | :--- |
| `spawn` | 0/1/2 standart | — | varsayılan |
| `SYS_OPEN` | ilk boş fd (`3+`) | +1 (aynı yolda paylaşılır) | `max_fds` denetlenir |
| `fork` | ebeveynden kopyalanır | her dolu fd için +1 | ebeveynden devralınır |
| `exec` | tümü kapanır, 0/1/2 yeniden kurulur | her fd için −1 | korunur |
| `SYS_EXIT` / `terminate` | tümü kapanır | her fd için −1 | — |
| `wait` / reap | kaynak iadesi tamamlanır | son referans içeriği serbest bırakır | — |

Bellek tarafında `record_frame`/`record_user_frame` her tahsiste limitleri
denetler; `mmap` canlı bölge baytlarını, `brk` heap ucunu karşılaştırır.
Kota aşımı tahsisi reddeder ve kullanımı değiştirmez (atomik).

### 2.12 Dosya Tanıtıcıları (15, 16, 17, 18)

Süreç başına sabit `FdTable` (16 fd, 0/1/2 standart akışlar rezerve) ve
çekirdek genelinde referans sayımlı `FileTable` (32 nesne).

* `SYS_OPEN(path, flags)`: yol `ptr`'den NUL'a kadar `copy_from_user` ile
alınır. İlk boş fd (`FIRST_USER_FD` = 3) döner. Kota (`max_fds`) doluysa `MAX`.
* `SYS_CLOSE(fd)`: nesne referansını bırakır; son referans içeriği serbest
bırakır. Geçersiz fd → `MAX`.
* `SYS_READ(fd, buf, len)`: `buf` yazılabilir ve COW olmayan kullanıcı sayfası
olmalı; okunan bayt sayısını döner.
* `SYS_RESINFO(out)`: 8 × `u64` — `max_frames, frame_count, max_user_frames,
user_frame_count, max_fds, used_fds, max_mmap_bytes, mmap_bytes`.
* `SYS_LSEEK(fd, offset, whence)`: `0`=SET, `1`=CUR, `2`=END; aralık dışı
konum `MAX` ile reddedilir.

`fork` fd'leri ve limitleri devralır (nesne referansları artar); `exec` tüm
kullanıcı fd'lerini kapatır, 0/1/2'yi yeniden kurar; çıkış/reap referansları
bırakır.

## 3. Hata Sözleşmesi

| Değer | Anlam |
| :--- | :--- |
| `0` | başarı (çağrıya göre) |
| `u64::MAX` | başarısız / desteklenmeyen çağrı numarası / geçersiz kullanıcı tamponu |
| `0` | `SYS_ALLOC_FRAME`/`SYS_MMAP` için "kaynak yok"; `SYS_OPEN` için "fd yok" (`fd = 0` hiçbir zaman dönmez) |

Syscall'lar kullanıcıya doğrudan sinyal üretmez; hata dönüş koduyla bildirilir.
`SYS_KILL` ile gönderilen fatal-olmayan sinyaller `pending_signals`'te bekler,
kullanıcı işleyicisine teslimat henüz uygulanmadı. Ring 3
hafıza hataları (`#PF`, `#GP`) ayrı yoldan `SIGSEGV` (`exit_code = -11`) ile
süreç sonlandırır.

Tüm kullanıcı işaretçileri `src/mm/uaccess.rs` ile **önce tam aralık olarak**
doğrulanır (atomiklik): NULL, taşma, kanonik olmayan adres, eşlenmemiş sayfa,
çekirdek yarısı, `PAGE_COW`/salt-okunur hedef ve 1 MiB üstü kopya reddedilir.
Bu yüzden hatalı bir `ptr` çekirdeği `#PF` ile düşürmez, `MAX` döner.

## 4. Copy-on-Write Ayrıntıları

1. `fork` sırasında ebeveynin **yazılabilir** kullanıcı sayfaları iki adres
   uzayında da `WRITABLE = 0` + `PAGE_COW` (PTE bit 9, yazılım biti) yapılır;
   aynı fiziksel çerçeve paylaşılır ve `vmm::mark_frame_shared` referans
   sayacını artırır. Salt-okunur sayfalar (kod, RO veri) doğrudan paylaşılır.
2. İlk yazma `#PF` üretir (`P=1, W/R=1, U/S=1`). `rust_fault`, CR2'yi okur ve
   `ProcessManager::handle_cow_fault` ile:
   * paylaşım tek eldeyse PTE'yi yerinde yazılabilir yapar (kopya yok),
   * değilse yeni çerçeve ayırır, içeriği kopyalar, PTE'yi yeniden eşler,
   * referans sayacını bırakır.
3. `fault_entry_rerun` stub'ı hata kodunu ve kaydedilmiş GPR kümesini atlayıp
   `iretq` ile **aynı komutu yeniden çalıştırır** (kullanıcıya görünmez).
4. Süreç sonunda `reap_process_resources` paylaşımlı çerçeveler için yalnız
   referans bırakır; son sahip fiziksel çerçeveyi PMM'e iade eder.

Defter kapasitesi `MAX_SHARED_FRAMES = 512`'dir; dolarsa fork çerçeveyi
özel kopya olarak ayırır (doğruluk korunur, COW avantajı kaybolur).

## 5. Bellek Düzeni (kullanıcı süreci)

| Aralık | İçerik | İzinler |
| :--- | :--- | :--- |
| `0x0000_0000_0040_0000` | Kod + RO veri (`PT_LOAD`, `PF_X`) | `USER`, NX=0 |
| `0x0000_0000_0060_0000` | Demo sonuç sayfası (test) | `USER_DATA` |
| `0x0000_0000_0100_0000` | **Heap** (`brk`) | `USER_DATA` (W, NX) |
| `0x0400_0000 – 0x1000_0000` | **mmap** (ASLR tabanı, 2 MiB hizalı) | `USER_DATA` |
| `0x0000_7FFF_FFFF_0000` | **Kullanıcı yığını** (4 sayfa, ASLR ile 0–256 KiB aşağı kaydırılır) | `USER_STACK` (W, NX) |
| yığın altındaki 1 sayfa | **Guard sayfası** — hiç eşlenmez | — |
| `0xFFFF_8000_0000_0000+` | Çekirdek (HHDM, PML4 üst yarı) | `USER` biti **yok** |

> **Gerçek donanım notu:** çekirdek, HHDM haritalaması 2 MiB sayfalarla
> yapıldığı için sembolik (HHDM) adreslerde PTE bayrağı değiştiremez ve
> kullanıcı çerçevelerine yalnızca HHDM üzerinden erişir. `fork`, COW ve
> `#PF` yolu her iki durumda da aynı PTE mantığını kullanır; ancak çekirdeğin
> kullanıcı bellek erişimi **her zaman** `translate_user` üzerinden olmalıdır.
> Bu kural `docs/HARDWARE_COMPATIBILITY.md` ile birlikte okunmalıdır.

## 6. Örnekler

```asm
; getpid
mov eax, 2
int 0x80                    ; rax = pid

; write(buf, len) — seri konsola yazar, fd argümanı yok
mov eax, 5
lea rdi, [rip + msg]
mov esi, msg_len
int 0x80

; brk(4096)
mov eax, 11
mov edi, 4096
int 0x80

; mmap(8192, 0)
mov eax, 12
mov edi, 8192
xor esi, esi
int 0x80                    ; rax = addr

; munmap(addr, 8192)
mov eax, 13
mov rdi, rax
mov esi, 8192
int 0x80

; fd = open("/etc/fd-test", O_RDONLY)
mov eax, 15
lea rdi, [rip + path]
xor esi, esi
int 0x80                    ; rax = fd

; read(fd, buf, 64)
mov eax, 6
mov rdi, rax                ; fd
lea rsi, [rip + buf]
mov edx, 64                 ; len (RDX)
int 0x80                    ; rax = okunan bayt

; lseek(fd, 0, SEEK_SET)
mov eax, 18
mov rdi, rax                ; fd
xor esi, esi                ; offset
xor edx, edx                ; whence
int 0x80                    ; rax = yeni konum

; close(fd)
mov eax, 16
mov rdi, rax
int 0x80

; kill(pid, SIGTERM) — RSI = sinyal no
mov eax, 19
mov rdi, target_pid
mov esi, 15
int 0x80

; exit(42)
mov eax, 1
mov edi, 42
int 0x80
```

## 7. QEMU Doğrulama İşaretleri

| İşaret | Test |
| :--- | :--- |
| `[PROC-TEST] ... fork/exec/wait yasam dongusu: BASARILI` | COW fork + exec + bloklanan wait |
| `[USER-MEM] ... BASARILI` | `brk`/`mmap`/`munmap`/`MEMINFO`/ASLR |
| `[FD-TEST] ... fork/exec ... BASARILI` | `open`/`read`/`lseek`/`close`, refcount, fork/exec, fd kotası |
| `[RLIMIT-TEST] ... BASARILI` | `RESINFO`, heap/mmap/fd/çerçeve kotası, fork mirası |
| `[COW-TEST] cocuk-exit=90 ... BASARILI` | Ring 3 yazma → COW kopyası |
| `[ASLR] yigin utu=0x...` | Yığın rastgele kaydırması |
| `[FD-TEST] ... BASARILI` | open/read/lseek/close, refcount, fork/exec, kota |
| `[RLIMIT-TEST] ... BASARILI` | RESINFO raporu, limitler ve fork mirası |
| `[P2] Kullanıcı yığını guard sayfası taşması yakalandı` | Guard sayfası #PF |
| `[KILL-TEST] kill/SIGSTOP/SIGCONT/SIGTERM: BASARILI` | `SYS_KILL` probe, suspend/resume, SIGTERM, kaynak temizliği |
| `[BIGPAGE] ... BASARILI` | 2 MiB büyük sayfa eşleme/geri alma doğrulaması |
| `[MZRAM] ... BASARILI` | Sıkıştırılmış (swap'siz) bellek deposu |
| `[MEM-STAT] total_kb=...` | Genişletilmiş bellek profili raporu |
| `[MEM-PROFILE] ...` / `[KHEAP] ...` | Açılışta PMM + kernel heap özeti |
| `[SCHED-STAT] slices=... cow=... shared=... susp=...` | Çalışan sistemde süreç/bellek izleme |
