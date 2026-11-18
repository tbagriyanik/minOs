//! QUIC akış/CRYPTO yeniden birleştirme tamponları.
//!
//! UDP kaybı ve yeniden iletim nedeniyle STREAM/CRYPTO verisi sırasız
//! ve çakışmalı gelebilir. Bu modül offset'li parçaları saklar,
//! tekrar gelen aralıkları eler ve uygulamaya yalnızca sürekli
//! kapsamı sıralı verir.

use alloc::vec::Vec;

/// Bir gelen STREAM'in yeniden birleştirme durumu.
pub struct StreamRx {
    /// Akış tanımlayıcısı (tekrar takibi için).
    pub id: u64,
    /// (offset, veri) parçaları — çakışmasız, sıralanmamış.
    chunks: Vec<(u64, Vec<u8>)>,
    /// Uygulamaya sıradaki verilecek bayt konumu (akış başından).
    next_out: u64,
    /// FIN'in işaret ettiği akış sonu konumu (tekrar gelen FIN'lerde
    /// en küçük değer korunur).
    fin_at: Option<u64>,
}

impl StreamRx {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            chunks: Vec::new(),
            next_out: 0,
            fin_at: None,
        }
    }

    /// `(offset, data)` parçasını ekler; mevcut parçalarla çakışan
    /// kısımları atlar (yeniden iletilen aynı aralıklar için güvenli).
    pub fn insert(&mut self, offset: u64, data: &[u8], fin: bool) {
        if fin {
            let at = offset.saturating_add(data.len() as u64);
            self.fin_at = Some(self.fin_at.map_or(at, |f| f.min(at)));
        }
        if data.is_empty() {
            return;
        }
        let end = offset.saturating_add(data.len() as u64);
        // Mevcut aralıklarla kesişmeyen boşlukları bul
        let mut covered: Vec<(u64, u64)> = self
            .chunks
            .iter()
            .map(|(o, d)| (*o, *o + d.len() as u64))
            .collect();
        covered.sort_by_key(|(o, _)| *o);
        let mut cur = offset;
        for (cs, ce) in covered {
            if ce <= cur {
                continue;
            }
            if cs >= end {
                break;
            }
            if cs > cur {
                // [cur, min(cs,end)) boşluk
                let gap_end = cs.min(end);
                let s = (cur - offset) as usize;
                let e = (gap_end - offset) as usize;
                if e > s {
                    self.chunks.push((cur, data[s..e].to_vec()));
                }
            }
            cur = cur.max(ce);
            if cur >= end {
                break;
            }
        }
        if cur < end {
            let s = (cur - offset) as usize;
            let e = (end - offset) as usize;
            if e > s {
                self.chunks.push((cur, data[s..e].to_vec()));
            }
        }
    }

    /// `next_out`'tan başlayan sürekli kapsamı çıkarır; boşluk
    /// sonrasındaki parçalar beklemeye devam eder. Dönen bool,
    /// FIN konumuna ulaşılıp tüm verinin tüketildiğini gösterir.
    pub fn take_contiguous(&mut self) -> (Vec<u8>, bool) {
        let mut out = Vec::new();
        loop {
            let mut best: Option<usize> = None;
            for (i, (o, d)) in self.chunks.iter().enumerate() {
                if *o <= self.next_out
                    && *o + d.len() as u64 > self.next_out
                    && best.is_none_or(|b| self.chunks[b].0 > *o)
                {
                    best = Some(i);
                }
            }
            let Some(i) = best else { break };
            let (o, d) = self.chunks.remove(i);
            let skip = (self.next_out - o) as usize;
            let avail = &d[skip.min(d.len())..];
            if avail.is_empty() {
                continue;
            }
            let take = avail.len() as u64;
            self.next_out += take;
            out.extend_from_slice(avail);
        }
        let done = self
            .fin_at
            .is_some_and(|f| self.next_out >= f && self.chunks.is_empty());
        (out, done)
    }
}

/// CRYPTO stream yeniden birleştirme: sıralı (offset, veri) parçaları.
pub struct CryptoRx {
    chunks: Vec<(u64, Vec<u8>)>,
    /// TLS'e zaten beslenen bayt sayısı (akış başından itibaren).
    pub fed: u64,
}

impl CryptoRx {
    pub fn new() -> Self {
        Self {
            chunks: Vec::new(),
            fed: 0,
        }
    }

    pub fn insert(&mut self, offset: u64, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        // Mevcut parçalar + yeni parça → offset sıralı süpürme.
        // Aynı aralık tekrar geldiğinde ilk kopya korunur (veri
        // yeniden iletimde birebir aynıdır).
        let mut all: Vec<(u64, Vec<u8>)> = core::mem::take(&mut self.chunks);
        all.push((offset, data.to_vec()));
        all.sort_by_key(|(o, _)| *o);
        let mut out: Vec<(u64, Vec<u8>)> = Vec::new();
        let mut cur_end = 0u64;
        let mut started = false;
        for (o, d) in all {
            let oe = o.saturating_add(d.len() as u64);
            if !started || o >= cur_end {
                out.push((o, d));
                cur_end = oe;
                started = true;
                continue;
            }
            // Çakışma: cur_end sonrası kalan kısmı koru
            let skip = (cur_end.saturating_sub(o)) as usize;
            if skip < d.len() {
                out.push((cur_end, d[skip..].to_vec()));
                cur_end = oe;
            }
        }
        out.retain(|(_, d)| !d.is_empty());
        self.chunks = out;
    }

    /// `fed` sonrasındaki sürekli kapsamın sonunu döndürür.
    pub fn contiguous_end(&self) -> u64 {
        let mut have = self.fed;
        loop {
            let mut advanced = false;
            for (o, d) in &self.chunks {
                let oe = *o + d.len() as u64;
                if *o <= have && oe > have {
                    have = oe;
                    advanced = true;
                }
            }
            if !advanced {
                return have;
            }
        }
    }

    /// `from..to` aralığını `out`'a kopyalar.
    pub fn copy_range(&self, from: u64, to: u64, out: &mut Vec<u8>) {
        for (o, d) in &self.chunks {
            let oe = *o + d.len() as u64;
            if oe <= from || *o >= to {
                continue;
            }
            let s = (from.max(*o) - *o) as usize;
            let e = (to.min(oe) - *o) as usize;
            out.extend_from_slice(&d[s..e]);
        }
    }

    /// `upto`'ya kadar olan veriyi tüketilmiş say.
    pub fn advance(&mut self, upto: u64) {
        self.fed = upto;
        let fed = self.fed;
        for (o, d) in self.chunks.iter_mut() {
            if *o < fed {
                let skip = (fed - *o) as usize;
                if skip >= d.len() {
                    d.clear();
                } else {
                    d.drain(..skip);
                }
                *o = fed;
            }
        }
        self.chunks.retain(|(_, d)| !d.is_empty());
    }
}

impl Default for CryptoRx {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn stream_rx_dedups_retransmitted_overlap() {
        let mut s = StreamRx::new(0);
        s.insert(0, b"hello ", false);
        // Yeniden iletilen aynı aralık + devamı
        s.insert(0, b"hello world", false);
        let (out, fin) = s.take_contiguous();
        assert_eq!(out, b"hello world");
        assert!(!fin);
    }

    #[test]
    fn stream_rx_gap_blocks_until_filled() {
        let mut s = StreamRx::new(0);
        s.insert(12, b"CDE", false); // boşluk: 0..12 eksik
        let (out, _) = s.take_contiguous();
        assert!(out.is_empty());
        s.insert(0, b"AB", false);
        s.insert(2, b"0123456789", false);
        let (out, _) = s.take_contiguous();
        assert_eq!(out, b"AB0123456789CDE");
    }

    #[test]
    fn stream_rx_fin_only_after_gap_closed() {
        let mut s = StreamRx::new(0);
        s.insert(5, b"", true); // boş FIN, akış sonu = 5
        s.insert(0, b"abc", false);
        let (out, fin) = s.take_contiguous();
        assert_eq!(out, b"abc");
        assert!(!fin); // 3..5 arası hâlâ eksik
        s.insert(3, b"de", false);
        let (out, fin) = s.take_contiguous();
        assert_eq!(out, b"de");
        assert!(fin);
    }

    #[test]
    fn crypto_rx_out_of_order_reassembly() {
        let mut c = CryptoRx::new();
        c.insert(4, b"efgh");
        c.insert(0, b"abcd");
        assert_eq!(c.contiguous_end(), 8);
        let mut out = Vec::new();
        c.copy_range(0, 8, &mut out);
        assert_eq!(out, b"abcdefgh");
        // Kısmi tekrar
        c.insert(2, b"cdef");
        let mut out2 = Vec::new();
        c.copy_range(0, 8, &mut out2);
        assert_eq!(out2, b"abcdefgh");
    }

    #[test]
    fn crypto_rx_advance_drops_consumed() {
        let mut c = CryptoRx::new();
        c.insert(0, b"hello world");
        c.advance(6);
        let mut out = Vec::new();
        c.copy_range(6, 11, &mut out);
        assert_eq!(out, b"world");
        assert_eq!(c.fed, 6);
    }

    #[test]
    fn stream_rx_reordered_chunks() {
        let mut s = StreamRx::new(0);
        s.insert(6, b"GHI", false);
        s.insert(0, b"ABC", false);
        s.insert(3, b"DEF", false);
        let (out, _) = s.take_contiguous();
        assert_eq!(out, b"ABCDEFGHI");
    }
}
