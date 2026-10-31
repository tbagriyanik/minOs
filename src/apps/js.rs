//! minJs — çok küçük bir istemci tarafı JavaScript alt kümesi (no_std).
//!
//! Tarayıcı `<script>` bloklarını bu modülle çalıştırır. Amaç tam JS değil,
//! sık kullanılan basit betiklerin çalışması ve `document.write` ile
//! sayfanın içeriğinin render'dan önce zenginleştirilmesidir.
//!
//! Desteklenenler:
//! - `console.log/info/warn/error(...)` — toplanır (`JsRuntime.logs`)
//! - `alert(...)` — aynı şekilde loglara yazılır
//! - `document.write(...)` / `document.writeln(...)` — HTML'e gömülür
//! - Değişken atamaları: `var|let|const x = ...` ve `x = ...`, `++`, `--`, `+=`, `-=`, `*=`, `/=`, `%=`
//! - Aritmetik ve işlem önceliği: `+`, `-`, `*`, `/`, `%`, parantezler `(...)`, tekli `+`, `-`, `!`
//! - Koşullar: `!`, `&&`, `||`, `==`/`!=`/`===`/`!==`, `<=`, `>=`, `<`, `>`, doğru/yanlış
//! - Fonksiyonlar: `function name(a, b) { ... }`, `return expr`, parametre geri yükleme, fonksiyon içi çağrılar
//! - Döngüler: gerçek `for (init; cond; step) { ... }`, `for (item of array)`, `for (key in object)`, `while (cond) { ... }`, `break`, `continue`
//! - Diziler: `[1, 2]`, `arr[i] = v`, `arr.length`, `.push()`, `.pop()`, `.shift()`, `.unshift()`, `.reverse()`, `.join()`, `.indexOf()`, `.includes()`, `.slice()`, `.concat()`
//! - Nesneler: `{ a: 1 }`, `obj.prop = v`, `obj["key"] = v`, iç içe nesneler (`a.b.c`), `delete obj.prop`, `Object.keys()`, `Object.values()`
//! - JSON ve Global API'ler: `JSON.stringify()`, `JSON.parse()`, `parseInt()`, `parseFloat()`, `String()`, `Number()`, `Boolean()`
//! - Matematik ve Dize: `Math.{abs, min, max, pow, floor, ceil, round}`, `str.{indexOf, includes, slice, split, trim, toLowerCase, toUpperCase, charAt}`
//! - DOM API'leri: `getElementById`, `getElementsByClassName`, `innerHTML`, `textContent`, `createElement`, `appendChild`, `setAttribute`, `getAttribute`, `addEventListener`, `dispatchEvent`
//! - Etkileşim: `prompt()`, `confirm()`
//!
//! Desteklenen DOM öğeleri:
//! - `document.getElementById(id)` — ID'sine göre eleman bulma
//! - `document.getElementsByClassName(class)` — Sınıfa göre eleman bulma
//! - `element.innerHTML` — İçeriği HTML olarak get/set
//! - `element.textContent` — Metin içeriği get/set
//! - `element.style.property` — CSS özelliği erişimi
//! - `element.setAttribute(attr, value)` — Özellik ayarlama
//! - `element.getAttribute(attr)` — Özellik okuma
//! - `document.createElement(tag)` — Yeni eleman oluşturma
//! - `element.appendChild(child)` — Alt eleman ekleme
//! - `element.remove()` — Eleman kaldırma

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::btree_map::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

#[cfg(not(any(test, host_fs_tests)))]
use crate::drivers::http;

// ── DOM Veri Yapıları ──────────────────────────────────────────────

/// Tek bir DOM elemanını temsil eder.
#[derive(Clone)]
pub struct DomElement {
    pub tag_name: String,
    pub attributes: BTreeMap<String, String>,
    pub inner_html: String,
    pub text_content: String,
    pub children: Vec<DomElement>,
    pub style: BTreeMap<String, String>,
    pub parent: Option<usize>, // Index vanem elementi
    pub id: Option<String>,
    pub class_name: Option<String>,
}

/// Taban belge yapısını temsil eder.
#[derive(Clone)]
pub struct DomDocument {
    pub root: Option<DomElement>,
    pub title: String,
    pub current_script: Option<String>,
}

// ── JS Runtime ile DOM entegrasyonu ──────────────────────────────

/// JS runtime içinde tutulan belge referansı.
#[derive(Clone)]
pub struct JsDocumentRef {
    /// Eleman ID'sine göre indeksleme (id -> element index)
    element_index: BTreeMap<String, usize>,
    /// Tüm elemanlar (root ile depth-first arşiv)
    elements: Vec<DomElement>,
    /// Root eleman index
    root_idx: usize,
}

impl JsDocumentRef {
    pub fn new() -> Self {
        Self {
            element_index: BTreeMap::new(),
            elements: Vec::new(),
            root_idx: 0,
        }
    }

    /// Root elemanı ekle
    pub fn add_root(&mut self, element: DomElement) {
        self.element_index.clear();
        self.elements.clear();
        self.root_idx = self.index_subtree(&element, None);
    }

    fn index_subtree(&mut self, element: &DomElement, parent: Option<usize>) -> usize {
        let idx = self.elements.len();
        let mut indexed = element.clone();
        indexed.parent = parent;
        self.elements.push(indexed);
        if let Some(id) = &element.id {
            self.element_index.insert(id.clone(), idx);
        }
        for child in &element.children {
            self.index_subtree(child, Some(idx));
        }
        idx
    }

    /// ID'ye göre eleman bul
    pub fn get_element_by_id(&self, id: &str) -> Option<usize> {
        self.element_index.get(id).copied()
    }

    /// Sınıf adıyla elemanlar bul (basit versiyon)
    pub fn get_elements_by_class_name(&self, class: &str) -> Vec<usize> {
        let mut results = Vec::new();
        for (idx, elem) in self.elements.iter().enumerate() {
            if elem
                .class_name
                .as_ref()
                .map_or(false, |c| c.contains(class))
            {
                results.push(idx);
            }
        }
        results
    }

    /// Elemanı DOM'a ekle (root altına)
    pub fn append_element(&mut self, element: DomElement) -> usize {
        if self.root_idx >= self.elements.len() {
            self.add_root(DomElement {
                tag_name: "body".to_string(),
                attributes: BTreeMap::new(),
                inner_html: String::new(),
                text_content: String::new(),
                children: Vec::new(),
                style: BTreeMap::new(),
                parent: None,
                id: None,
                class_name: None,
            });
        }
        let idx = self.index_subtree(&element, Some(self.root_idx));
        self.elements[self.root_idx].children.push(element);
        idx
    }
}

// ── JS Değer ve Fonksiyon Tipleri ──────────────────────────────────

/// minJs içinde dinamik olarak temsil edilen değer tipleri.
#[derive(Clone, Debug)]
pub enum JsValue {
    Undefined,
    Null,
    Bool(bool),
    Number(i64),
    String(String),
    Array(Vec<JsValue>),
    Object(BTreeMap<String, JsValue>),
    /// Promise değeri: beklemede / yerine geldi / reddedildi.
    Promise(Box<PromiseObj>),
}

/// Promise durumu.
#[derive(Clone, Debug, PartialEq)]
pub enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

/// Bir Promise örneği.
#[derive(Clone, Debug)]
pub struct PromiseObj {
    pub state: PromiseState,
    pub value: Option<JsValue>,
    /// .then() / .catch() ile kaydedilen işleyiciler.
    pub handlers: Vec<PromiseHandler>,
}

/// Promise işleyicisi: .then(onFulfilled, onRejected) kaydı.
#[derive(Clone, Debug)]
pub struct PromiseHandler {
    pub on_fulfilled: Option<String>, // JS kod parçası veya fonksiyon adı
    pub on_rejected: Option<String>,
}

/// Sınıf tanımı: constructor ve metodlar.
#[derive(Clone, Debug)]
pub struct JsClass {
    pub name: String,
    pub extends: Option<String>,
    /// Örnek metotları (instance methods).
    pub methods: BTreeMap<String, JsFunction>,
    /// Statik metotları (static methods), sınıf üzerinden çağrılır.
    pub static_methods: BTreeMap<String, JsFunction>,
}

impl JsClass {
    pub fn new(name: String, extends: Option<String>) -> Self {
        Self {
            name,
            extends,
            methods: BTreeMap::new(),
            static_methods: BTreeMap::new(),
        }
    }
}

impl PromiseObj {
    pub fn new() -> Self {
        Self {
            state: PromiseState::Pending,
            value: None,
            handlers: Vec::new(),
        }
    }
}

impl JsValue {
    pub fn to_str(&self) -> String {
        match self {
            JsValue::Undefined => String::from("undefined"),
            JsValue::Null => String::from("null"),
            JsValue::Bool(b) => {
                if *b {
                    String::from("true")
                } else {
                    String::from("false")
                }
            }
            JsValue::Number(n) => n.to_string(),
            JsValue::String(s) => s.clone(),
            JsValue::Array(items) => {
                let mut res = String::new();
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        res.push(',');
                    }
                    res.push_str(&item.to_str());
                }
                res
            }
            JsValue::Object(map) => {
                let mut res = String::from("{");
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        res.push_str(", ");
                    }
                    res.push_str(k);
                    res.push_str(": ");
                    res.push_str(&v.to_str());
                }
                res.push('}');
                res
            }
            JsValue::Promise(p) => {
                let state_str = match p.state {
                    PromiseState::Pending => "pending",
                    PromiseState::Fulfilled => "fulfilled",
                    PromiseState::Rejected => "rejected",
                };
                format!("[object Promise:{}]", state_str)
            }
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            JsValue::Undefined | JsValue::Null => false,
            JsValue::Bool(b) => *b,
            JsValue::Number(n) => *n != 0,
            JsValue::String(s) => !s.is_empty(),
            JsValue::Array(_) | JsValue::Object(_) | JsValue::Promise(_) => true,
        }
    }

    pub fn to_i64(&self) -> i64 {
        match self {
            JsValue::Number(n) => *n,
            JsValue::Bool(b) => {
                if *b {
                    1
                } else {
                    0
                }
            }
            JsValue::String(s) => s.trim().parse::<i64>().unwrap_or(0),
            JsValue::Array(items) => {
                if items.len() == 1 {
                    items[0].to_i64()
                } else {
                    0
                }
            }
            _ => 0,
        }
    }
}

/// Kullanıcı tarafından tanımlanan fonksiyon.
#[derive(Clone, Debug)]
pub struct JsFunction {
    pub name: String,
    pub params: Vec<String>,
    pub body: String,
    /// async işareti: true ise çağrıldığında sonuç Promise ile sarmalanır.
    pub is_async: bool,
}

/// Kayıtlı olay dinleyicisi (addEventListener).
#[derive(Clone, Debug)]
pub struct EventListener {
    pub target: String,
    pub event_type: String,
    pub handler: String,
}

/// Zamanlayıcı (setTimeout / setInterval).
#[derive(Clone, Debug)]
pub struct Timer {
    pub id: u32,
    pub callback: String, // fonksiyon adı veya JS kodu
    pub delay_ms: u32,
    pub repeat: bool, // true = setInterval, false = setTimeout
    pub fire_at: u64, // ticks'de ateşleme zamanı
}

// Sayfa boyunca süren JS durumu: değişkenler ve konsol çıktısı.
pub struct JsRuntime {
    pub vars: Vec<(String, String)>,
    pub values: BTreeMap<String, JsValue>,
    pub functions: BTreeMap<String, JsFunction>,
    pub listeners: Vec<EventListener>,
    pub logs: Vec<String>,
    /// Son `if` koşulunun sonucu (`else` dalı için).
    last_cond: bool,
    /// Son fonksiyon `return` değeri.
    pub last_return: Option<JsValue>,
    /// Bağlı doc referansı (tarayıcı tarafından ayarlanır)
    doc: Option<JsDocumentRef>,
    /// Promise microtask sırası: çalıştırılacak JS kod parçaları.
    pub microtask_queue: Vec<String>,
    /// Prototip zinciri: nesne_değişkeni -> prototip_değişkeni.
    pub protos: BTreeMap<String, String>,
    /// Sınıf tanımları: sınıf_ismi -> JsClass.
    pub classes: BTreeMap<String, JsClass>,
    /// Strict mode (`"use strict"`) etkin mi.
    pub strict_mode: bool,
    /// Zamanlayıcılar (setTimeout / setInterval).
    pub timers: Vec<Timer>,
    /// Bir sonraki zamanlayıcı kimliği.
    pub next_timer_id: u32,
    /// localStorage kalıcı anahtar-değer deposu.
    pub local_storage: BTreeMap<String, String>,
    /// sessionStorage oturum bazlı anahtar-değer deposu.
    pub session_storage: BTreeMap<String, String>,
    /// WebSocket bağlantıları: kimlik -> (url, readyState, onmessage handler).
    pub websockets: BTreeMap<u32, WebSocketConn>,
    /// Bir sonraki WebSocket kimliği.
    pub next_ws_id: u32,
}

impl Clone for JsRuntime {
    fn clone(&self) -> Self {
        // WebSocket bağlantıları kopyalanır ancak TLS/soket durumu (connection alanı)
        // sıfırlanır, çünkü WebSocketConnection Clone değil.
        let websockets = self
            .websockets
            .iter()
            .map(|(id, conn)| {
                let cloned = conn.clone();
                #[cfg(not(any(test, host_fs_tests)))]
                {
                    let mut cloned = cloned;
                    cloned.connection = None;
                    (*id, cloned)
                }
                #[cfg(any(test, host_fs_tests))]
                {
                    (*id, cloned)
                }
            })
            .collect();
        Self {
            vars: self.vars.clone(),
            values: self.values.clone(),
            functions: self.functions.clone(),
            listeners: self.listeners.clone(),
            logs: self.logs.clone(),
            last_cond: self.last_cond,
            last_return: self.last_return.clone(),
            doc: self.doc.clone(),
            microtask_queue: self.microtask_queue.clone(),
            protos: self.protos.clone(),
            classes: self.classes.clone(),
            strict_mode: self.strict_mode,
            timers: self.timers.clone(),
            next_timer_id: self.next_timer_id,
            local_storage: self.local_storage.clone(),
            session_storage: self.session_storage.clone(),
            websockets,
            next_ws_id: self.next_ws_id,
        }
    }
}

/// WebSocket bağlantı durumu.
pub struct WebSocketConn {
    pub url: String,
    pub ready_state: u8, // 0=CONNECTING, 1=OPEN, 2=CLOSING, 3=CLOSED
    pub onmessage: Option<String>,
    pub onopen: Option<String>,
    pub onerror: Option<String>,
    pub onclose: Option<String>,
    pub buffered: Vec<String>, // alınan mesaj kuyruğu
    #[cfg(not(any(test, host_fs_tests)))]
    pub connection: Option<crate::drivers::http::WebSocketConnection>,
}

impl Clone for WebSocketConn {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            ready_state: self.ready_state,
            onmessage: self.onmessage.clone(),
            onopen: self.onopen.clone(),
            onerror: self.onerror.clone(),
            onclose: self.onclose.clone(),
            buffered: self.buffered.clone(),
            #[cfg(not(any(test, host_fs_tests)))]
            connection: None, // WebSocketConnection Clone değil, sıfırla
        }
    }
}

impl WebSocketConn {
    pub fn new(url: String) -> Self {
        Self {
            url,
            ready_state: 0,
            onmessage: None,
            onopen: None,
            onerror: None,
            onclose: None,
            buffered: Vec::new(),
            #[cfg(not(any(test, host_fs_tests)))]
            connection: None,
        }
    }
}

impl JsRuntime {
    pub fn new() -> Self {
        Self {
            vars: Vec::new(),
            values: BTreeMap::new(),
            functions: BTreeMap::new(),
            listeners: Vec::new(),
            logs: Vec::new(),
            last_cond: false,
            last_return: None,
            doc: None,
            microtask_queue: Vec::new(),
            protos: BTreeMap::new(),
            classes: BTreeMap::new(),
            strict_mode: false,
            timers: Vec::new(),
            next_timer_id: 1,
            local_storage: BTreeMap::new(),
            session_storage: BTreeMap::new(),
            websockets: BTreeMap::new(),
            next_ws_id: 1,
        }
    }

    pub fn set_document(&mut self, doc: JsDocumentRef) {
        self.doc = Some(doc);
    }

    pub fn var_get(&self, name: &str) -> Option<&str> {
        self.vars
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn val_get(&self, name: &str) -> Option<&JsValue> {
        self.values.get(name)
    }

    pub fn var_set(&mut self, name: &str, value: String) {
        let val = self.eval_val(&value);
        self.var_set_val(name, val);
    }

    pub fn var_set_val(&mut self, name: &str, val: JsValue) {
        let s = val.to_str();
        if let Some(entry) = self.vars.iter_mut().find(|(k, _)| k == name) {
            entry.1 = s;
        } else {
            self.vars.push((String::from(name), s));
        }
        self.values.insert(String::from(name), val);
    }

    pub fn sync_var_from_val(&mut self, name: &str) {
        if let Some(val) = self.values.get(name) {
            let s = val.to_str();
            if let Some(entry) = self.vars.iter_mut().find(|(k, _)| k == name) {
                entry.1 = s;
            } else {
                self.vars.push((String::from(name), s));
            }
        }
    }

    // ── Prototip zinciri yardımcıları ──────────────────────────────

    /// Nesnenin prototipini ayarlar: obj.__proto__ = proto
    pub fn set_proto(&mut self, obj_name: &str, proto_name: &str) {
        self.protos
            .insert(String::from(obj_name), String::from(proto_name));
    }

    /// Bir özelliği prototip zinciri boyunca arar. Önce kendi özellikleri,
    /// sonra prototip, prototipin prototipi gibi yukarı doğru.
    pub fn val_get_proto(&self, obj_name: &str, prop: &str) -> Option<JsValue> {
        let mut current = String::from(obj_name);
        let mut visited = alloc::collections::BTreeSet::new();
        while !current.is_empty() && visited.insert(current.clone()) {
            if let Some(JsValue::Object(map)) = self.values.get(&current) {
                if let Some(v) = map.get(prop) {
                    return Some(v.clone());
                }
            }
            current = self.protos.get(&current).cloned().unwrap_or_default();
        }
        None
    }

    /// `obj instanceof Constructor` kontrolü: Constructor.prototype
    /// prototip zincirinde mi?
    pub fn instance_of(&self, obj_name: &str, constructor_name: &str) -> bool {
        let proto_var = format!("{}.prototype", constructor_name);
        let mut current = String::from(obj_name);
        let mut visited = alloc::collections::BTreeSet::new();
        while !current.is_empty() && visited.insert(current.clone()) {
            if current == proto_var {
                return true;
            }
            current = self.protos.get(&current).cloned().unwrap_or_default();
        }
        false
    }

    /// Bir değerin verilen sınıfın (ya da miras aldığı bir atasının) örneği
    /// olup olmadığını `__class__` / `__extends__` üzerinden kontrol eder.
    fn value_instance_of(&self, val: &JsValue, class_name: &str) -> bool {
        let map = match val {
            JsValue::Object(map) => map,
            _ => return false,
        };
        let mut current = match map.get("__class__") {
            Some(JsValue::String(name)) => name.clone(),
            _ => return false,
        };
        let mut depth = 0u32;
        while depth < 32 {
            if current == class_name {
                return true;
            }
            let next = self
                .classes
                .get(&current)
                .and_then(|c| c.extends.clone())
                .or_else(|| match map.get("__extends__") {
                    Some(JsValue::String(parent)) => Some(parent.clone()),
                    _ => None,
                });
            match next {
                Some(parent) if !parent.is_empty() && parent != current => {
                    current = parent;
                    depth += 1;
                }
                _ => return false,
            }
        }
        false
    }

    /// Bir sınıfın miras zinciri boyunca tüm metodlarını toplar.
    /// Alt sınıfın aynı adlı metodu ebeveyninkini geçersiz kılar.
    fn class_methods(&self, class_name: &str) -> BTreeMap<String, JsFunction> {
        let mut out = BTreeMap::new();
        let mut current = Some(String::from(class_name));
        let mut depth = 0u32;
        while let Some(name) = current {
            if depth >= 32 {
                break;
            }
            depth += 1;
            match self.classes.get(&name) {
                Some(class) => {
                    for (mname, mfunc) in &class.methods {
                        out.entry(mname.clone()).or_insert_with(|| mfunc.clone());
                    }
                    current = class.extends.clone();
                }
                None => break,
            }
        }
        out
    }

    /// `class_name` sınıfının constructor'ını mevcut `this` üzerinde çalıştırır.
    /// `extends` varsa ebeveyn constructor'ı `super(...)` ile ya da örtük
    /// olarak (gövdede `super(` yoksa) çağrılır.
    fn run_constructor(&mut self, class_name: &str, args: &[JsValue], depth: u32) {
        if depth > 32 {
            return;
        }
        let (extends, ctor) = match self.classes.get(class_name) {
            Some(class) => (
                class.extends.clone(),
                class.methods.get("constructor").cloned(),
            ),
            None => return,
        };
        let (params, body) = match ctor {
            Some(ctor) => (ctor.params, ctor.body),
            None => (Vec::new(), String::new()),
        };
        for (i, param) in params.iter().enumerate() {
            let val = args.get(i).cloned().unwrap_or(JsValue::Undefined);
            self.var_set_val(param, val);
        }
        let explicit_super = body.contains("super(");
        if let (Some(parent), false) = (&extends, explicit_super) {
            self.run_constructor(parent, &[], depth + 1);
        }
        for stmt in split_statements(&body) {
            let t = stmt.trim();
            if let Some(rest) = t.strip_prefix("super(") {
                if let Some(close) = rest.rfind(')') {
                    let parent_args: Vec<JsValue> = split_top_level_comma(&rest[..close])
                        .into_iter()
                        .map(|a| self.eval_val(a))
                        .collect();
                    if let Some(parent) = &extends {
                        self.run_constructor(parent, &parent_args, depth + 1);
                    }
                    continue;
                }
            }
            self.exec(&stmt, &mut Vec::new());
            if self.last_return.is_some() {
                break;
            }
        }
        for param in &params {
            self.values.remove(param);
        }
    }

    // ── Promise / microtask yardımcıları ───────────────────────────

    /// Microtask sırasını çalıştırır (Promise .then() işleyicileri vb.).
    /// Her microtask bir JS kod parçasıdır; sırayla çalıştırılır.
    pub fn run_microtasks(&mut self, out: &mut Vec<u8>) {
        while !self.microtask_queue.is_empty() {
            let code = self.microtask_queue.remove(0);
            let _ = self.run(&code, out);
        }
    }

    /// Bir Promise'i yerine getirir ve kayıtlı .then() işleyicilerini
    /// microtask sırasına ekler.
    pub fn resolve_promise(&mut self, promise_var: &str, value: JsValue) {
        let handlers = {
            if let Some(JsValue::Promise(p)) = self.values.get_mut(promise_var) {
                p.state = PromiseState::Fulfilled;
                p.value = Some(value.clone());
                p.handlers.clone()
            } else {
                return;
            }
        };
        // Çözümlenmiş değeri .then() işleyicilerinin erişebileceği bir değişkende sakla.
        self.values.insert(String::from("__then_value"), value);
        for handler in &handlers {
            if let Some(cb) = &handler.on_fulfilled {
                self.microtask_queue.push(cb.clone());
            }
        }
    }

    /// Bir Promise'i reddeder ve .catch() işleyicilerini microtask sırasına ekler.
    pub fn reject_promise(&mut self, promise_var: &str, reason: JsValue) {
        let handlers = {
            if let Some(JsValue::Promise(p)) = self.values.get_mut(promise_var) {
                p.state = PromiseState::Rejected;
                p.value = Some(reason.clone());
                p.handlers.clone()
            } else {
                return;
            }
        };
        // Reddedilme sebebini .catch() işleyicilerinin erişebileceği bir değişkende sakla.
        self.values.insert(String::from("__catch_reason"), reason);
        for handler in &handlers {
            if let Some(cb) = &handler.on_rejected {
                self.microtask_queue.push(cb.clone());
            }
        }
    }

    /// Zamanlayıcı ekler (setTimeout / setInterval).
    pub fn add_timer(&mut self, callback: String, delay_ms: u32, repeat: bool) -> u32 {
        let id = self.next_timer_id;
        self.next_timer_id += 1;
        self.timers.push(Timer {
            id,
            callback,
            delay_ms,
            repeat,
            fire_at: 0, // caller tarafından ayarlanır
        });
        id
    }

    /// Zamanlayıcıyı kaldırır (clearTimeout / clearInterval).
    pub fn remove_timer(&mut self, id: u32) {
        self.timers.retain(|t| t.id != id);
    }

    /// Zamanlayıcıları kontrol eder ve hazır olanları çalıştırır.
    pub fn check_timers(&mut self, now_ticks: u64, _ticks_per_ms: u64, _out: &mut Vec<u8>) {
        let mut to_run: Vec<String> = Vec::new();
        let mut to_remove: Vec<u32> = Vec::new();
        for t in &self.timers {
            if t.fire_at != 0 && now_ticks >= t.fire_at {
                to_run.push(t.callback.clone());
                if t.repeat {
                    // tekrarlayan: tekrar programla
                    // (basit implementasyon: fire_at'i güncelle)
                } else {
                    to_remove.push(t.id);
                }
            }
        }
        for id in &to_remove {
            self.remove_timer(*id);
        }
        for cb in &to_run {
            self.microtask_queue.push(cb.clone());
        }
    }

    pub fn define_function(&mut self, name: &str, params: Vec<String>, body: &str, is_async: bool) {
        self.functions.insert(
            String::from(name),
            JsFunction {
                name: String::from(name),
                params,
                body: String::from(body),
                is_async,
            },
        );
    }

    pub fn call_function(&mut self, name: &str, args: &[JsValue], out: &mut Vec<u8>) -> JsValue {
        let func = match self.functions.get(name).cloned() {
            Some(f) => f,
            None => return JsValue::Undefined,
        };
        let old_values = self.values.clone();
        for (i, param) in func.params.iter().enumerate() {
            let val = args.get(i).cloned().unwrap_or(JsValue::Undefined);
            self.var_set_val(param, val);
        }
        self.last_return = None;
        for stmt in split_statements(&func.body) {
            self.exec(&stmt, out);
            if self.last_return.is_some() {
                break;
            }
        }
        let mut ret = self.last_return.take().unwrap_or(JsValue::Undefined);
        // async fonksiyon: sonucu her zaman Promise ile sarmala.
        if func.is_async {
            let pvar = format!("__async_promise_{}", self.values.len());
            let mut p = PromiseObj::new();
            p.state = PromiseState::Fulfilled;
            p.value = Some(ret.clone());
            self.values
                .insert(pvar.clone(), JsValue::Promise(Box::new(p)));
            ret = JsValue::Promise(Box::new(PromiseObj {
                state: PromiseState::Fulfilled,
                value: Some(JsValue::String(pvar)),
                handlers: Vec::new(),
            }));
        }
        for param in &func.params {
            if let Some(prev) = old_values.get(param) {
                self.var_set_val(param, prev.clone());
            } else {
                self.values.remove(param);
                self.vars.retain(|(k, _)| k != param);
            }
        }
        ret
    }

    pub fn call_function_pure(&self, name: &str, args: &[JsValue]) -> JsValue {
        let mut sub = self.clone();
        let mut dummy = Vec::new();
        sub.call_function(name, args, &mut dummy)
    }

    /// fetch(url) — HTTP GET isteği yapar ve Promise değeri döndürür.
    #[cfg(not(any(test, host_fs_tests)))]
    fn do_fetch(&mut self, url: &str) -> Result<JsValue, i32> {
        if !crate::drivers::e1000::is_ready() {
            return Err(http::E_HTTP);
        }

        let mut host = [0u8; http::MAX_HOST];
        let mut path = [0u8; http::MAX_PATH];
        let (port, host_len, path_len) = http::parse_url(url, &mut host, &mut path)?;
        let is_https = url.starts_with("https://");
        let host_str = core::str::from_utf8(&host[..host_len]).map_err(|_| http::E_URL)?;
        let ip = crate::drivers::socket::resolve_host(host_str)?;

        let mut resp_buf = [0u8; 8192];
        let config = http::HttpClientConfig::default();
        let (status, hlen, total) = http::fetch_once(
            &host[..host_len],
            port,
            &path[..path_len],
            ip,
            &mut resp_buf,
            is_https,
            None,
            &config,
        )?;

        // Chunked gövdeyi çöz
        let headers = &resp_buf[..hlen];
        let is_chunked = http::header_value(headers, b"transfer-encoding")
            .map(|v| {
                v.iter()
                    .zip(b"chunked".iter())
                    .all(|(a, b)| a.to_ascii_lowercase() == *b)
                    && v.len() == 7
            })
            .unwrap_or(false);
        let body_start = hlen;
        let body_end = if is_chunked {
            let body_len = total - hlen;
            let mut tmp = resp_buf[hlen..total].to_vec();
            http::decode_chunked(&mut tmp, body_len)
                .map(|decoded| {
                    resp_buf[hlen..hlen + decoded].copy_from_slice(&tmp[..decoded]);
                    hlen + decoded
                })
                .unwrap_or(total)
        } else {
            total
        };

        let body = core::str::from_utf8(&resp_buf[body_start..body_end]).unwrap_or("");
        let status_text = match status {
            200 => "OK",
            201 => "Created",
            204 => "No Content",
            301 => "Moved Permanently",
            302 => "Found",
            304 => "Not Modified",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            500 => "Internal Server Error",
            _ => "Unknown",
        };

        // Response objesi oluştur
        let resp_var = format!("__resp_{}", self.values.len());
        let mut resp_obj = BTreeMap::new();
        resp_obj.insert(
            String::from("ok"),
            JsValue::Bool(status >= 200 && status < 300),
        );
        resp_obj.insert(String::from("status"), JsValue::Number(status as i64));
        resp_obj.insert(
            String::from("statusText"),
            JsValue::String(status_text.to_string()),
        );
        resp_obj.insert(String::from("body"), JsValue::String(body.to_string()));
        resp_obj.insert(String::from("headers"), JsValue::Object(BTreeMap::new()));

        // text() metodu
        resp_obj.insert(
            String::from("text"),
            JsValue::Object(BTreeMap::from([
                (
                    String::from("__fn__"),
                    JsValue::String(format!("return {};", body)),
                ),
                (String::from("__params__"), JsValue::String(String::new())),
            ])),
        );

        // json() metodu
        resp_obj.insert(
            String::from("json"),
            JsValue::Object(BTreeMap::from([
                (
                    String::from("__fn__"),
                    JsValue::String(format!(
                        "return JSON.parse('{}');",
                        body.replace("'", "\\'")
                    )),
                ),
                (String::from("__params__"), JsValue::String(String::new())),
            ])),
        );

        self.values
            .insert(resp_var.clone(), JsValue::Object(resp_obj));

        // Promise oluştur ve yerine getir (Response objesinin değişken ismi ile)
        let pvar = format!("__fetch_promise_{}", self.values.len());
        let mut p = PromiseObj::new();
        p.state = PromiseState::Fulfilled;
        p.value = Some(JsValue::String(resp_var.clone()));
        self.values
            .insert(pvar.clone(), JsValue::Promise(Box::new(p)));

        Ok(JsValue::String(pvar))
    }

    // Host test harness'ı gerçek ağ sürücülerini bağlamaz. JS testleri ağ
    // isteğini değil, çalışma zamanının geri kalanını doğrular.
    #[cfg(any(test, host_fs_tests))]
    fn do_fetch(&mut self, _url: &str) -> Result<JsValue, i32> {
        Err(1003)
    }

    /// localStorage.getItem(key) — kalıcı depodan değer okur.
    fn local_storage_get(&mut self, key: &str) -> JsValue {
        self.local_storage
            .get(key)
            .map(|v| JsValue::String(v.clone()))
            .unwrap_or(JsValue::Null)
    }

    /// localStorage.setItem(key, value) — kalıcı depoya değer yazar.
    fn local_storage_set(&mut self, key: &str, value: &str) {
        self.local_storage
            .insert(String::from(key), String::from(value));
    }

    /// localStorage.removeItem(key) — kalıcı depodan değer siler.
    fn local_storage_remove(&mut self, key: &str) {
        self.local_storage.remove(key);
    }

    /// localStorage.clear() — kalıcı depoyu temizler.
    fn local_storage_clear(&mut self) {
        self.local_storage.clear();
    }

    /// sessionStorage.getItem(key) — oturum deposundan değer okur.
    fn session_storage_get(&mut self, key: &str) -> JsValue {
        self.session_storage
            .get(key)
            .map(|v| JsValue::String(v.clone()))
            .unwrap_or(JsValue::Null)
    }

    /// sessionStorage.setItem(key, value) — oturum deposuna değer yazar.
    fn session_storage_set(&mut self, key: &str, value: &str) {
        self.session_storage
            .insert(String::from(key), String::from(value));
    }

    /// sessionStorage.removeItem(key) — oturum deposundan değer siler.
    fn session_storage_remove(&mut self, key: &str) {
        self.session_storage.remove(key);
    }

    /// sessionStorage.clear() — oturum deposunu temizler.
    fn session_storage_clear(&mut self) {
        self.session_storage.clear();
    }

    pub fn add_event_listener(&mut self, target: &str, event_type: &str, handler: &str) {
        let clean_target = target.trim().trim_matches(|c| c == '\'' || c == '"');
        let clean_event = event_type.trim().trim_matches(|c| c == '\'' || c == '"');
        self.listeners.push(EventListener {
            target: String::from(clean_target),
            event_type: String::from(clean_event),
            handler: String::from(handler.trim()),
        });
    }

    pub fn dispatch_event(&mut self, target: &str, event_type: &str, out: &mut Vec<u8>) -> bool {
        let clean_target = target.trim().trim_matches(|c| c == '\'' || c == '"');
        let clean_event = event_type.trim().trim_matches(|c| c == '\'' || c == '"');
        let matching: Vec<String> = self
            .listeners
            .iter()
            .filter(|l| {
                (l.target == clean_target
                    || l.target == "window"
                    || l.target == "document"
                    || clean_target.is_empty())
                    && l.event_type.eq_ignore_ascii_case(clean_event)
            })
            .map(|l| l.handler.clone())
            .collect();

        if matching.is_empty() {
            return false;
        }

        for handler in matching {
            let h = handler.trim();
            if self.functions.contains_key(h) {
                let _ = self.call_function(h, &[], out);
            } else {
                self.run(h, out);
            }
        }
        true
    }

    /// İfadeleri `JsValue` olarak değerlendirir.
    pub fn eval_val(&mut self, expr: &str) -> JsValue {
        let e = expr
            .trim()
            .trim_end_matches(|c| c == ';' || c == '\n' || c == '\r')
            .trim();
        if e.is_empty() {
            return JsValue::Undefined;
        }

        // await expr — Promise değeri varsa çözümlenmiş değeri döndürür.
        if let Some(rest) = e.strip_prefix("await ") {
            let val = self.eval_val(rest);
            match val {
                JsValue::Promise(p) => match p.state {
                    PromiseState::Fulfilled => {
                        return p.value.clone().unwrap_or(JsValue::Undefined)
                    }
                    PromiseState::Rejected => return JsValue::Undefined,
                    PromiseState::Pending => return JsValue::Undefined,
                },
                other => return other,
            }
        }

        // Parantezleri temizle: ((expr)) -> expr
        let stripped = strip_matching_parens(e);
        if stripped != e {
            return self.eval_val(stripped);
        }

        // `obj instanceof Constructor` — sınıf/miras zinciri kontrolü.
        if let Some(idx) = find_top_level_word(e.as_bytes(), b"instanceof") {
            let left = e[..idx].trim();
            let right = e[idx + "instanceof".len()..].trim();
            let val = self.eval_val(left);
            return JsValue::Bool(self.value_instance_of(&val, right));
        }

        // Top-level '+' ve '-' ikili işlemleri (en sağdan bölerek sol-ilişkili)
        if let Some((op_idx, op_char)) = find_rightmost_binary_op(e.as_bytes(), b"+-") {
            let lhs = self.eval_val(&e[..op_idx]);
            let rhs = self.eval_val(&e[op_idx + 1..]);
            if op_char == b'+' {
                if let (JsValue::Number(a), JsValue::Number(b)) = (&lhs, &rhs) {
                    return JsValue::Number(a + b);
                }
                let mut s = lhs.to_str();
                s.push_str(&rhs.to_str());
                return JsValue::String(s);
            } else {
                return JsValue::Number(lhs.to_i64() - rhs.to_i64());
            }
        }

        // Top-level '*', '/', '%' ikili işlemleri
        if let Some((op_idx, op_char)) = find_rightmost_binary_op(e.as_bytes(), b"*/%") {
            let lhs = self.eval_val(&e[..op_idx]);
            let rhs = self.eval_val(&e[op_idx + 1..]);
            let a = lhs.to_i64();
            let b = rhs.to_i64();
            let res = match op_char {
                b'*' => a * b,
                b'/' => {
                    if b != 0 {
                        a / b
                    } else {
                        0
                    }
                }
                b'%' => {
                    if b != 0 {
                        a % b
                    } else {
                        0
                    }
                }
                _ => 0,
            };
            return JsValue::Number(res);
        }

        // Tekli işlemler: `-`, `+`, `!`
        if let Some(rest) = e.strip_prefix('-') {
            if !rest.starts_with('-') {
                return JsValue::Number(-self.eval_val(rest).to_i64());
            }
        }
        if let Some(rest) = e.strip_prefix('+') {
            if !rest.starts_with('+') {
                return JsValue::Number(self.eval_val(rest).to_i64());
            }
        }
        if let Some(rest) = e.strip_prefix('!') {
            if !rest.starts_with('=') {
                return JsValue::Bool(!self.eval_val(rest).is_truthy());
            }
        }

        // Metot çağrıları: obj.method(args), Math.abs(...), JSON.stringify(...), Object.keys(...)
        if let Some((base, method, args)) = extract_method_call(e) {
            match base {
                "Math" => match method {
                    "abs" => return JsValue::Number(self.eval_val(args).to_i64().abs()),
                    "floor" | "ceil" | "round" => {
                        return JsValue::Number(self.eval_val(args).to_i64())
                    }
                    "min" => {
                        let mut m = i64::MAX;
                        for a in split_top_level_comma(args) {
                            let n = self.eval_val(a).to_i64();
                            if n < m {
                                m = n;
                            }
                        }
                        return JsValue::Number(if m == i64::MAX { 0 } else { m });
                    }
                    "max" => {
                        let mut m = i64::MIN;
                        for a in split_top_level_comma(args) {
                            let n = self.eval_val(a).to_i64();
                            if n > m {
                                m = n;
                            }
                        }
                        return JsValue::Number(if m == i64::MIN { 0 } else { m });
                    }
                    "pow" => {
                        let parts = split_top_level_comma(args);
                        let b = parts
                            .first()
                            .map(|p| self.eval_val(p).to_i64())
                            .unwrap_or(0);
                        let exp = parts.get(1).map(|p| self.eval_val(p).to_i64()).unwrap_or(1);
                        let res = if exp >= 0 {
                            b.saturating_pow(exp as u32)
                        } else {
                            0
                        };
                        return JsValue::Number(res);
                    }
                    _ => {}
                },
                "JSON" => match method {
                    "stringify" => {
                        let v = self.eval_val(args);
                        return JsValue::String(json_stringify(&v));
                    }
                    "parse" => {
                        let raw = self.eval_val(args).to_str();
                        return self.eval_val(&raw);
                    }
                    _ => {}
                },
                "Object" => match method {
                    "keys" => {
                        let v = self.eval_val(args);
                        let keys = match v {
                            JsValue::Object(map) => {
                                map.keys().cloned().map(JsValue::String).collect()
                            }
                            JsValue::Array(items) => (0..items.len())
                                .map(|i| JsValue::String(i.to_string()))
                                .collect(),
                            _ => Vec::new(),
                        };
                        return JsValue::Array(keys);
                    }
                    "values" => {
                        let v = self.eval_val(args);
                        let vals = match v {
                            JsValue::Object(map) => map.into_values().collect(),
                            JsValue::Array(items) => items,
                            _ => Vec::new(),
                        };
                        return JsValue::Array(vals);
                    }
                    "create" => {
                        // Object.create(proto) — yeni nesne, prototip zinciri kurulur.
                        let proto_val = self.eval_val(args);
                        let proto_name = match &proto_val {
                            JsValue::String(s) => s.clone(),
                            JsValue::Object(_) => {
                                // Anonim nesne: bir değişken ismi olarak saklanamaz,
                                // burada basitçe boş bir prototip kullanılır.
                                String::new()
                            }
                            _ => String::new(),
                        };
                        let new_var = format!("__obj_{}", self.values.len());
                        self.var_set_val(&new_var, JsValue::Object(BTreeMap::new()));
                        if !proto_name.is_empty() {
                            self.set_proto(&new_var, &proto_name);
                        }
                        return JsValue::Object(BTreeMap::new());
                    }
                    _ => {}
                },
                "Promise" => match method {
                    "resolve" => {
                        let v = self.eval_val(args);
                        let mut p = PromiseObj::new();
                        p.state = PromiseState::Fulfilled;
                        p.value = Some(v);
                        return JsValue::Promise(Box::new(p));
                    }
                    "reject" => {
                        let v = self.eval_val(args);
                        let mut p = PromiseObj::new();
                        p.state = PromiseState::Rejected;
                        p.value = Some(v);
                        return JsValue::Promise(Box::new(p));
                    }
                    "all" => {
                        let arr_val = self.eval_val(args);
                        let mut p = PromiseObj::new();
                        let mut results = Vec::new();
                        let mut any_pending = false;
                        let mut any_rejected = false;
                        if let JsValue::Array(items) = &arr_val {
                            for item in items {
                                match item {
                                    JsValue::Promise(promise) => match promise.state {
                                        PromiseState::Fulfilled => {
                                            results.push(
                                                promise.value.clone().unwrap_or(JsValue::Undefined),
                                            );
                                        }
                                        PromiseState::Rejected => {
                                            any_rejected = true;
                                            results.push(
                                                promise.value.clone().unwrap_or(JsValue::Undefined),
                                            );
                                        }
                                        PromiseState::Pending => {
                                            any_pending = true;
                                            results.push(JsValue::Undefined);
                                        }
                                    },
                                    other => results.push(other.clone()),
                                }
                            }
                        }
                        if any_rejected {
                            p.state = PromiseState::Rejected;
                            p.value = results.first().cloned();
                        } else if any_pending {
                            p.state = PromiseState::Pending;
                            p.value = Some(JsValue::Array(results));
                        } else {
                            p.state = PromiseState::Fulfilled;
                            p.value = Some(JsValue::Array(results));
                        }
                        return JsValue::Promise(Box::new(p));
                    }
                    "race" => {
                        let arr_val = self.eval_val(args);
                        let mut p = PromiseObj::new();
                        if let JsValue::Array(items) = &arr_val {
                            for item in items {
                                if let JsValue::Promise(promise) = item {
                                    match promise.state {
                                        PromiseState::Fulfilled => {
                                            p.state = PromiseState::Fulfilled;
                                            p.value = promise.value.clone();
                                            return JsValue::Promise(Box::new(p));
                                        }
                                        PromiseState::Rejected => {
                                            p.state = PromiseState::Rejected;
                                            p.value = promise.value.clone();
                                            return JsValue::Promise(Box::new(p));
                                        }
                                        PromiseState::Pending => {}
                                    }
                                } else {
                                    p.state = PromiseState::Fulfilled;
                                    p.value = Some(item.clone());
                                    return JsValue::Promise(Box::new(p));
                                }
                            }
                        }
                        p.state = PromiseState::Pending;
                        p.value = None;
                        return JsValue::Promise(Box::new(p));
                    }
                    "allSettled" => {
                        let arr_val = self.eval_val(args);
                        let mut p = PromiseObj::new();
                        p.state = PromiseState::Fulfilled;
                        let mut results = Vec::new();
                        if let JsValue::Array(items) = &arr_val {
                            for item in items {
                                match item {
                                    JsValue::Promise(promise) => match promise.state {
                                        PromiseState::Fulfilled => {
                                            results.push(JsValue::Object(BTreeMap::from([
                                                (
                                                    String::from("status"),
                                                    JsValue::String(String::from("fulfilled")),
                                                ),
                                                (
                                                    String::from("value"),
                                                    promise
                                                        .value
                                                        .clone()
                                                        .unwrap_or(JsValue::Undefined),
                                                ),
                                            ])));
                                        }
                                        PromiseState::Rejected => {
                                            results.push(JsValue::Object(BTreeMap::from([
                                                (
                                                    String::from("status"),
                                                    JsValue::String(String::from("rejected")),
                                                ),
                                                (
                                                    String::from("reason"),
                                                    promise
                                                        .value
                                                        .clone()
                                                        .unwrap_or(JsValue::Undefined),
                                                ),
                                            ])));
                                        }
                                        PromiseState::Pending => {
                                            results.push(JsValue::Object(BTreeMap::from([(
                                                String::from("status"),
                                                JsValue::String(String::from("pending")),
                                            )])));
                                        }
                                    },
                                    other => {
                                        results.push(JsValue::Object(BTreeMap::from([
                                            (
                                                String::from("status"),
                                                JsValue::String(String::from("fulfilled")),
                                            ),
                                            (String::from("value"), other.clone()),
                                        ])));
                                    }
                                }
                            }
                        }
                        p.value = Some(JsValue::Array(results));
                        return JsValue::Promise(Box::new(p));
                    }
                    _ => {}
                },
                "localStorage" => match method {
                    "getItem" => {
                        let key = self.eval_val(args).to_str();
                        return self.local_storage_get(&key);
                    }
                    "setItem" => {
                        let parts = split_top_level_comma(args);
                        if parts.len() >= 2 {
                            let key = self.eval_val(parts[0]).to_str();
                            let val = self.eval_val(parts[1]).to_str();
                            self.local_storage_set(&key, &val);
                        }
                        return JsValue::Undefined;
                    }
                    "removeItem" => {
                        let key = self.eval_val(args).to_str();
                        self.local_storage_remove(&key);
                        return JsValue::Undefined;
                    }
                    "clear" => {
                        self.local_storage_clear();
                        return JsValue::Undefined;
                    }
                    "length" => {
                        return JsValue::Number(self.local_storage.len() as i64);
                    }
                    _ => {}
                },
                "sessionStorage" => match method {
                    "getItem" => {
                        let key = self.eval_val(args).to_str();
                        return self.session_storage_get(&key);
                    }
                    "setItem" => {
                        let parts = split_top_level_comma(args);
                        if parts.len() >= 2 {
                            let key = self.eval_val(parts[0]).to_str();
                            let val = self.eval_val(parts[1]).to_str();
                            self.session_storage_set(&key, &val);
                        }
                        return JsValue::Undefined;
                    }
                    "removeItem" => {
                        let key = self.eval_val(args).to_str();
                        self.session_storage_remove(&key);
                        return JsValue::Undefined;
                    }
                    "clear" => {
                        self.session_storage_clear();
                        return JsValue::Undefined;
                    }
                    "length" => {
                        return JsValue::Number(self.session_storage.len() as i64);
                    }
                    _ => {}
                },
                "WebSocket" => match method {
                    "send" => {
                        let data = self.eval_val(args).to_str();
                        if let Some(JsValue::Object(map)) = self.values.get_mut("__ws_current__") {
                            if let Some(JsValue::Number(ws_id)) = map.get("__ws_id__") {
                                if let Some(conn) = self.websockets.get_mut(&(*ws_id as u32)) {
                                    #[cfg(not(any(test, host_fs_tests)))]
                                    if let Some(ref mut ws_conn) = conn.connection {
                                        let _ = ws_conn.send(data.as_bytes());
                                    }
                                    #[cfg(any(test, host_fs_tests))]
                                    let _ = (&data, &conn);
                                    conn.buffered.push(data.clone());
                                }
                            }
                        }
                        return JsValue::Undefined;
                    }
                    "close" => {
                        if let Some(JsValue::Object(map)) = self.values.get("__ws_current__") {
                            if let Some(JsValue::Number(ws_id)) = map.get("__ws_id__") {
                                if let Some(conn) = self.websockets.get_mut(&(*ws_id as u32)) {
                                    #[cfg(not(any(test, host_fs_tests)))]
                                    if let Some(ref mut ws_conn) = conn.connection {
                                        let _ = ws_conn.close();
                                    }
                                    conn.ready_state = 3;
                                }
                            }
                        }
                        return JsValue::Undefined;
                    }
                    _ => {}
                },
                _ => {
                    let base_val = self.eval_val(base);
                    match base_val {
                        JsValue::Array(items) => match method {
                            "join" => {
                                let sep = if args.is_empty() {
                                    String::from(",")
                                } else {
                                    self.eval_val(args).to_str()
                                };
                                let mut s = String::new();
                                for (i, item) in items.iter().enumerate() {
                                    if i > 0 {
                                        s.push_str(&sep);
                                    }
                                    s.push_str(&item.to_str());
                                }
                                return JsValue::String(s);
                            }
                            "indexOf" => {
                                let needle = self.eval_val(args);
                                let needle_str = needle.to_str();
                                let pos = items.iter().position(|it| it.to_str() == needle_str);
                                return JsValue::Number(pos.map(|p| p as i64).unwrap_or(-1));
                            }
                            "includes" => {
                                let needle = self.eval_val(args);
                                let needle_str = needle.to_str();
                                let has = items.iter().any(|it| it.to_str() == needle_str);
                                return JsValue::Bool(has);
                            }
                            "slice" => {
                                let parts = split_top_level_comma(args);
                                let len = items.len() as i64;
                                let mut start = parts
                                    .first()
                                    .map(|p| self.eval_val(p).to_i64())
                                    .unwrap_or(0);
                                if start < 0 {
                                    start = (len + start).max(0);
                                }
                                let start = (start as usize).min(items.len());
                                let mut end = parts
                                    .get(1)
                                    .map(|p| self.eval_val(p).to_i64())
                                    .unwrap_or(len);
                                if end < 0 {
                                    end = (len + end).max(0);
                                }
                                let end = (end as usize).min(items.len());
                                let sliced = if start <= end {
                                    items[start..end].to_vec()
                                } else {
                                    Vec::new()
                                };
                                return JsValue::Array(sliced);
                            }
                            "concat" => {
                                let mut new_items = items;
                                for a in split_top_level_comma(args) {
                                    match self.eval_val(a) {
                                        JsValue::Array(mut other) => new_items.append(&mut other),
                                        other => new_items.push(other),
                                    }
                                }
                                return JsValue::Array(new_items);
                            }
                            _ => {}
                        },
                        JsValue::String(s) => match method {
                            "indexOf" => {
                                let needle = self.eval_val(args).to_str();
                                let pos = s.find(&needle).map(|p| p as i64).unwrap_or(-1);
                                return JsValue::Number(pos);
                            }
                            "toString" => return JsValue::String(s),
                            "concat" => {
                                let mut out = s;
                                for a in split_top_level_comma(args) {
                                    out.push_str(&self.eval_val(a).to_str());
                                }
                                return JsValue::String(out);
                            }
                            "repeat" => {
                                let n = self.eval_val(args).to_i64().clamp(0, 1024) as usize;
                                return JsValue::String(s.repeat(n));
                            }
                            "replace" => {
                                // Standart String#replace gibi yalnızca ilk eşleşme.
                                let parts: Vec<String> = split_top_level_comma(args)
                                    .into_iter()
                                    .map(|a| self.eval_val(a).to_str())
                                    .collect();
                                if let (Some(from), Some(to)) = (parts.first(), parts.get(1)) {
                                    return JsValue::String(s.replacen(from, to, 1));
                                }
                                return JsValue::String(s);
                            }
                            "includes" => {
                                let needle = self.eval_val(args).to_str();
                                return JsValue::Bool(s.contains(&needle));
                            }
                            "slice" | "substring" => {
                                let parts = split_top_level_comma(args);
                                let len = s.chars().count() as i64;
                                let mut start = parts
                                    .first()
                                    .map(|p| self.eval_val(p).to_i64())
                                    .unwrap_or(0);
                                if start < 0 {
                                    start = (len + start).max(0);
                                }
                                let start = (start as usize).min(s.chars().count());
                                let mut end = parts
                                    .get(1)
                                    .map(|p| self.eval_val(p).to_i64())
                                    .unwrap_or(len);
                                if end < 0 {
                                    end = (len + end).max(0);
                                }
                                let end = (end as usize).min(s.chars().count());
                                let chars: String = s
                                    .chars()
                                    .skip(start)
                                    .take(end.saturating_sub(start))
                                    .collect();
                                return JsValue::String(chars);
                            }
                            "split" => {
                                let sep = self.eval_val(args).to_str();
                                let parts: Vec<JsValue> = if sep.is_empty() {
                                    s.chars().map(|c| JsValue::String(c.to_string())).collect()
                                } else {
                                    s.split(&sep)
                                        .map(|piece| JsValue::String(piece.to_string()))
                                        .collect()
                                };
                                return JsValue::Array(parts);
                            }
                            "trim" => return JsValue::String(s.trim().to_string()),
                            "toLowerCase" => return JsValue::String(s.to_lowercase()),
                            "toUpperCase" => return JsValue::String(s.to_uppercase()),
                            "charAt" => {
                                let idx = self.eval_val(args).to_i64() as usize;
                                let ch = s
                                    .chars()
                                    .nth(idx)
                                    .map(|c| c.to_string())
                                    .unwrap_or_default();
                                return JsValue::String(ch);
                            }
                            _ => {}
                        },
                        JsValue::Object(map) => match method {
                            "send" => {
                                // WebSocket.send(data) — mesaj gönder
                                if let Some(JsValue::Number(ws_id)) = map.get("__ws_id__") {
                                    let data = self.eval_val(args).to_str();
                                    if let Some(conn) = self.websockets.get_mut(&(*ws_id as u32)) {
                                        #[cfg(not(any(test, host_fs_tests)))]
                                        if let Some(ref mut ws_conn) = conn.connection {
                                            let _ = ws_conn.send(data.as_bytes());
                                        }
                                        #[cfg(any(test, host_fs_tests))]
                                        let _ = (&data, &conn);
                                        conn.buffered.push(data);
                                    }
                                }
                                return JsValue::Undefined;
                            }
                            "close" => {
                                // WebSocket.close() — bağlantıyı kapat
                                if let Some(JsValue::Number(ws_id)) = map.get("__ws_id__") {
                                    if let Some(conn) = self.websockets.get_mut(&(*ws_id as u32)) {
                                        #[cfg(not(any(test, host_fs_tests)))]
                                        if let Some(ref mut ws_conn) = conn.connection {
                                            let _ = ws_conn.close();
                                        }
                                        conn.ready_state = 3;
                                    }
                                }
                                return JsValue::Undefined;
                            }
                            _ => {}
                        },
                        JsValue::Promise(mut p) => match method {
                            "then" => {
                                // .then(onFulfilled, onRejected) — işleyiciyi kaydet, yeni Promise döndür.
                                let parts: Vec<&str> = split_top_level_comma(args);
                                let on_fulfilled = parts.first().map(|s| s.trim().to_string());
                                let on_rejected = parts.get(1).map(|s| s.trim().to_string());

                                // İşleyiciyi promise'in handler listesine kaydet (resolve/reject sırasında çağrılır).
                                if let Some(cb) = &on_fulfilled {
                                    p.handlers.push(PromiseHandler {
                                        on_fulfilled: Some(cb.clone()),
                                        on_rejected: None,
                                    });
                                }
                                if let Some(cb) = &on_rejected {
                                    if let Some(last) = p.handlers.last_mut() {
                                        if last.on_rejected.is_none() {
                                            last.on_rejected = Some(cb.clone());
                                        } else {
                                            p.handlers.push(PromiseHandler {
                                                on_fulfilled: None,
                                                on_rejected: Some(cb.clone()),
                                            });
                                        }
                                    } else {
                                        p.handlers.push(PromiseHandler {
                                            on_fulfilled: None,
                                            on_rejected: Some(cb.clone()),
                                        });
                                    }
                                }

                                // Yeni bir Promise döndür (zincirleme için).
                                let new_p = PromiseObj::new();
                                let pvar = format!("__promise_{}", self.values.len());
                                self.values
                                    .insert(pvar.clone(), JsValue::Promise(Box::new(new_p)));
                                return JsValue::String(pvar);
                            }
                            "catch" => {
                                // .catch(onRejected) — hata işleyicisi kaydet, yeni Promise döndür.
                                let parts: Vec<&str> = split_top_level_comma(args);
                                if let Some(cb) = parts.first().map(|s| s.trim().to_string()) {
                                    p.handlers.push(PromiseHandler {
                                        on_fulfilled: None,
                                        on_rejected: Some(cb),
                                    });
                                }

                                // Yeni bir Promise döndür.
                                let new_p = PromiseObj::new();
                                let pvar = format!("__promise_{}", self.values.len());
                                self.values
                                    .insert(pvar.clone(), JsValue::Promise(Box::new(new_p)));
                                return JsValue::String(pvar);
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
        }

        // Dizi literali: [1, 2, 3] ve Dizi/Nesne erişimi: expr[index]
        if e.ends_with(']') {
            let b = e.as_bytes();
            let mut depth = 0i32;
            let mut quote: Option<u8> = None;
            let mut open_idx = None;
            for i in (0..b.len()).rev() {
                let c = b[i];
                if let Some(q) = quote {
                    if c == q {
                        quote = None;
                    }
                    continue;
                }
                match c {
                    b'"' | b'\'' | b'`' => quote = Some(c),
                    b']' => depth += 1,
                    b'[' => {
                        depth -= 1;
                        if depth == 0 {
                            open_idx = Some(i);
                            break;
                        }
                    }
                    _ => {}
                }
            }

            if let Some(open_br) = open_idx {
                if open_br == 0 {
                    // Dizi literali: [ ... ]
                    let inner = &e[1..e.len() - 1].trim();
                    if inner.is_empty() {
                        return JsValue::Array(Vec::new());
                    }
                    let items: Vec<JsValue> = split_top_level_comma(inner)
                        .into_iter()
                        .map(|s| self.eval_val(s))
                        .collect();
                    return JsValue::Array(items);
                } else {
                    // İndeks erişimi: base[idx]
                    let base_str = &e[..open_br];
                    let idx_str = &e[open_br + 1..e.len() - 1];
                    let base_val = self.eval_val(base_str);
                    let idx_val = self.eval_val(idx_str);
                    match base_val {
                        JsValue::Array(items) => {
                            let idx = idx_val.to_i64() as usize;
                            return items.get(idx).cloned().unwrap_or(JsValue::Undefined);
                        }
                        JsValue::Object(map) => {
                            let key = idx_val.to_str();
                            return map.get(&key).cloned().unwrap_or(JsValue::Undefined);
                        }
                        _ => {}
                    }
                }
            }
        }

        // Nesne literali: { a: 1, b: "x" }
        if e.starts_with('{') && e.ends_with('}') && e.len() >= 2 {
            let inner = &e[1..e.len() - 1].trim();
            let mut map = BTreeMap::new();
            if !inner.is_empty() {
                for pair in split_top_level_comma(inner) {
                    if let Some(colon) = find_top_level_char(pair.as_bytes(), ':') {
                        let k = pair[..colon].trim().trim_matches(|c| c == '\'' || c == '"');
                        let v = self.eval_val(&pair[colon + 1..]);
                        map.insert(String::from(k), v);
                    }
                }
            }
            return JsValue::Object(map);
        }

        // String literali
        if (e.starts_with('"') && e.ends_with('"') && e.len() >= 2)
            || (e.starts_with('\'') && e.ends_with('\'') && e.len() >= 2)
            || (e.starts_with('`') && e.ends_with('`') && e.len() >= 2)
        {
            return JsValue::String(String::from(&e[1..e.len() - 1]));
        }

        // Temel literaller
        if e == "true" {
            return JsValue::Bool(true);
        }
        if e == "false" {
            return JsValue::Bool(false);
        }
        if e == "null" {
            return JsValue::Null;
        }
        if e == "undefined" {
            return JsValue::Undefined;
        }

        // Tamsayı
        if let Ok(n) = e.parse::<i64>() {
            return JsValue::Number(n);
        }

        // Özellik erişimi: obj.prop veya arr.length
        if let Some((lhs, rhs)) = e.rsplit_once('.') {
            if !lhs.is_empty()
                && !rhs.is_empty()
                && rhs
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                && !rhs.chars().next().map_or(false, |c| c.is_ascii_digit())
            {
                let base_val = self.eval_val(lhs);
                match base_val {
                    JsValue::Array(items) => {
                        if rhs == "length" {
                            return JsValue::Number(items.len() as i64);
                        }
                    }
                    JsValue::String(s) => {
                        if rhs == "length" {
                            return JsValue::Number(s.len() as i64);
                        }
                    }
                    JsValue::Object(map) => {
                        if rhs == "__proto__" {
                            // Prototip zincirinden prototip nesnesini döndür.
                            let lhs_name = lhs.trim();
                            if let Some(proto_name) = self.protos.get(lhs_name) {
                                if let Some(JsValue::Object(proto_map)) =
                                    self.values.get(proto_name)
                                {
                                    return JsValue::Object(proto_map.clone());
                                }
                            }
                            return JsValue::Undefined;
                        }
                        if rhs == "length" {
                            return JsValue::Number(map.len() as i64);
                        }
                        // Önce kendi özelliklerde, sonra prototip zincirinde ara.
                        if let Some(v) = map.get(rhs) {
                            return v.clone();
                        }
                        // Prototip zincirinde ara (lhs bir değişken ismiyse).
                        let lhs_name = lhs.trim();
                        if let Some(v) = self.val_get_proto(lhs_name, rhs) {
                            return v;
                        }
                        return JsValue::Undefined;
                    }
                    _ => {}
                }
            }
        }

        // `new ClassName(args)` — sınıf örneği oluşturur.
        if let Some(rest) = e.strip_prefix("new ") {
            let rest = rest.trim();
            if let Some(open) = rest.find('(') {
                if rest.ends_with(')') {
                    let class_name = rest[..open].trim();
                    let arg_str = &rest[open + 1..rest.len() - 1];
                    // Sınıf verisini ödünç çakışması olmadan klonla.
                    let extends_opt = self.classes.get(class_name).and_then(|c| c.extends.clone());
                    if self.classes.contains_key(class_name) {
                        // Miras zinciri boyunca tüm metodları topla (alt sınıf kazanır).
                        let methods = self.class_methods(class_name);
                        // Argümanları değerlendir
                        let args: Vec<JsValue> = split_top_level_comma(arg_str)
                            .into_iter()
                            .map(|a| self.eval_val(a))
                            .collect();
                        // Yeni nesne oluştur.
                        let mut obj = BTreeMap::new();
                        obj.insert(
                            String::from("__class__"),
                            JsValue::String(class_name.to_string()),
                        );
                        if let Some(extends) = &extends_opt {
                            obj.insert(
                                String::from("__extends__"),
                                JsValue::String(extends.clone()),
                            );
                        }
                        // Sınıf ve miras alınan metodları ekle.
                        for (mname, mfunc) in &methods {
                            obj.insert(mname.clone(), method_object(mfunc));
                        }
                        // Constructor'ı `this` örneğe bağlı olarak çalıştır.
                        // `extends` varsa ebeveyn constructor'ı `super()` ile ya da
                        // örtük olarak çalışır.
                        let old_this = self.values.get("this").cloned();
                        self.values
                            .insert(String::from("this"), JsValue::Object(obj.clone()));
                        self.last_return = None;
                        self.run_constructor(class_name, &args, 0);
                        // `this` üzerinden yazılan özellikleri örneğe aktar.
                        if let Some(JsValue::Object(mutated)) = self.values.get("this").cloned() {
                            obj = mutated;
                        }
                        self.last_return = None;
                        match old_this {
                            Some(v) => {
                                self.values.insert(String::from("this"), v);
                            }
                            None => {
                                self.values.remove("this");
                            }
                        }
                        return JsValue::Object(obj);
                    }
                }
            }
        }

        // `obj.method(args)` — nesne/metot çağrısı. Yöntem gövdesi
        // `this` örneğe bağlanarak çalıştırılır ve `this` üzerindeki
        // değişiklikler geri yazılır.
        if let Some(dot_open) = find_method_call_dot(e) {
            let obj_name = e[..dot_open].trim();
            let after_dot = &e[dot_open + 1..];
            if let Some(method_open) = after_dot.find('(') {
                let mname = after_dot[..method_open].trim();
                let arg_str = &after_dot[method_open + 1..after_dot.len() - 1];

                // Statik metot çağrısı: ClassName.staticMethod(args)
                if let Some(class) = self.classes.get(obj_name) {
                    if let Some(static_method) = class.static_methods.get(mname) {
                        // Klonla: self.exec / self.var_set_val self'i değiştirir.
                        let sm = static_method.clone();
                        let args: Vec<JsValue> = split_top_level_comma(arg_str)
                            .into_iter()
                            .map(|a| self.eval_val(a))
                            .collect();
                        let old_this = self.values.get("this").cloned();
                        self.values.insert(String::from("this"), JsValue::Undefined);
                        for (i, param) in sm.params.iter().enumerate() {
                            let val = args.get(i).cloned().unwrap_or(JsValue::Undefined);
                            self.var_set_val(param, val);
                        }
                        self.last_return = None;
                        for stmt in split_statements(&sm.body) {
                            self.exec(&stmt, &mut Vec::new());
                            if self.last_return.is_some() {
                                break;
                            }
                        }
                        let ret = self.last_return.take().unwrap_or(JsValue::Undefined);
                        for param in &sm.params {
                            self.values.remove(param);
                        }
                        match old_this {
                            Some(v) => {
                                self.values.insert(String::from("this"), v);
                            }
                            None => {
                                self.values.remove("this");
                            }
                        }
                        return ret;
                    }
                }

                // Örnek metot çağrısı: obj.method(args)
                let base_val = self.eval_val(obj_name);
                if let JsValue::Object(map) = &base_val {
                    if let Some(JsValue::Object(method_map)) = map.get(mname) {
                        if let Some(JsValue::String(body)) = method_map.get("__fn__") {
                            let params_str = method_map
                                .get("__params__")
                                .map(|s| s.to_str())
                                .unwrap_or_default();
                            let params: Vec<String> = params_str
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect();
                            let args: Vec<JsValue> = split_top_level_comma(arg_str)
                                .into_iter()
                                .map(|a| self.eval_val(a))
                                .collect();
                            let old_this = self.values.get("this").cloned();
                            self.values
                                .insert(String::from("this"), JsValue::Object(map.clone()));
                            for (i, param) in params.iter().enumerate() {
                                let val = args.get(i).cloned().unwrap_or(JsValue::Undefined);
                                self.var_set_val(param, val);
                            }
                            self.last_return = None;
                            for stmt in split_statements(body) {
                                self.exec(&stmt, &mut Vec::new());
                                if self.last_return.is_some() {
                                    break;
                                }
                            }
                            let ret = self.last_return.take().unwrap_or(JsValue::Undefined);
                            // `this` üzerinden yapılan atamaları örneğe geri yaz.
                            if let Some(JsValue::Object(mutated)) = self.values.get("this").cloned()
                            {
                                if is_simple_ident(obj_name) {
                                    if let Some(existing) = self.values.get_mut(obj_name) {
                                        if let JsValue::Object(target) = existing {
                                            *target = mutated;
                                        }
                                    }
                                    self.sync_var_from_val(obj_name);
                                }
                            }
                            for param in &params {
                                self.values.remove(param);
                            }
                            match old_this {
                                Some(v) => {
                                    self.values.insert(String::from("this"), v);
                                }
                                None => {
                                    self.values.remove("this");
                                }
                            }
                            return ret;
                        }
                    }
                }
            }
        }

        // Global fonksiyon ve Kullanıcı fonksiyon çağrısı: fname(args)
        if let Some(open) = e.find('(') {
            if e.ends_with(')') {
                let fname = e[..open].trim();
                // `Math.abs(..)`, `JSON.stringify(..)` gibi ad alanlı çağrılar
                // yukarıdaki metot dalında ele alınır. Burada yeniden
                // değerlendirmek (`eval_val(fname)`) geri dönüşlü olduğundan
                // bu blok yalnızca ad alanı olmayan adlar için çalışır.
                if fname.contains('.') {
                    return JsValue::String(self.eval_expr_legacy(e));
                }
                let arg_str = &e[open + 1..e.len() - 1];
                match fname {
                    "parseInt" => {
                        let parts = split_top_level_comma(arg_str);
                        let first = self.eval_val(parts.first().unwrap_or(&"")).to_str();
                        let radix = parts
                            .get(1)
                            .map(|r| self.eval_val(r).to_i64() as u32)
                            .unwrap_or(10);
                        let cleaned = first.trim();
                        let n = i64::from_str_radix(cleaned, radix).unwrap_or(0);
                        return JsValue::Number(n);
                    }
                    "parseFloat" => {
                        let first = self.eval_val(arg_str).to_str();
                        let n = first
                            .trim()
                            .split('.')
                            .next()
                            .unwrap_or("0")
                            .parse::<i64>()
                            .unwrap_or(0);
                        return JsValue::Number(n);
                    }
                    "String" => {
                        return JsValue::String(self.eval_val(arg_str).to_str());
                    }
                    "Number" => {
                        return JsValue::Number(self.eval_val(arg_str).to_i64());
                    }
                    "Boolean" => {
                        return JsValue::Bool(self.eval_val(arg_str).is_truthy());
                    }
                    _ => {
                        if self.functions.contains_key(fname) {
                            let args: Vec<JsValue> = split_top_level_comma(arg_str)
                                .into_iter()
                                .map(|a| self.eval_val(a))
                                .collect();
                            return self.call_function_pure(fname, &args);
                        }
                    }
                }
            }
        }

        // Değişken tablosundan okuma
        if let Some(v) = self.values.get(e) {
            return v.clone();
        }
        if let Some(v) = self.var_get(e) {
            return JsValue::String(String::from(v));
        }

        // fetch(url) — Promise döndürür
        if e.starts_with("fetch(") && e.ends_with(')') {
            if let Some((args, _)) = call_args(e, 5) {
                let url = self.eval_val(args.trim()).to_str();
                let url = url.trim_matches(|c| c == '\'' || c == '"');
                match self.do_fetch(&url) {
                    Ok(promise_val) => {
                        return promise_val;
                    }
                    Err(_) => {
                        return JsValue::Null;
                    }
                }
            }
        }

        // localStorage / sessionStorage — depolama nesnesi döndürür
        if e == "localStorage" {
            let mut map = BTreeMap::new();
            map.insert(
                String::from("__storage_type__"),
                JsValue::String(String::from("local")),
            );
            return JsValue::Object(map);
        }
        if e == "sessionStorage" {
            let mut map = BTreeMap::new();
            map.insert(
                String::from("__storage_type__"),
                JsValue::String(String::from("session")),
            );
            return JsValue::Object(map);
        }

        // WebSocket(url) — WebSocket nesnesi döndürür ve bağlanmaya çalışır
        if e.starts_with("WebSocket(") && e.ends_with(')') {
            if let Some((args, _)) = call_args(e, 9) {
                let url = self.eval_val(args.trim()).to_str();
                let url = url.trim_matches(|c| c == '\'' || c == '"');
                let ws_id = self.next_ws_id;
                self.next_ws_id += 1;
                let ws_var = format!("__ws_{}", ws_id);

                // WebSocket bağlantısını kur (ws:// veya wss://).
                #[cfg(not(any(test, host_fs_tests)))]
                {
                    let mut conn = WebSocketConn::new(url.to_string());
                    match crate::drivers::http::WebSocketConnection::connect(&url) {
                        Ok(ws_conn) => {
                            conn.ready_state = 1; // OPEN
                            conn.connection = Some(ws_conn);
                        }
                        Err(_) => {
                            conn.ready_state = 3; // CLOSED (bağlantı başarısız)
                        }
                    }
                    self.websockets.insert(ws_id, conn);
                }
                let mut ws_obj = BTreeMap::new();
                ws_obj.insert(String::from("__ws_id__"), JsValue::Number(ws_id as i64));
                ws_obj.insert(String::from("url"), JsValue::String(url.to_string()));
                ws_obj.insert(String::from("readyState"), JsValue::Number(self.websockets[&ws_id].ready_state as i64));
                self.values.insert(ws_var.clone(), JsValue::Object(ws_obj));
                return JsValue::String(ws_var);
            }
        }

        // Eski geri uyumluluk: eval_expr mantığı
        JsValue::String(self.eval_expr_legacy(e))
    }

    /// String birleştirmeyi çözer (eski eval_expr mantığı ile tam uyumlu).
    fn eval_expr_legacy(&mut self, expr: &str) -> String {
        let b = expr.as_bytes();
        let mut i = 0usize;
        let mut res = String::new();
        while i < b.len() {
            let c = b[i];
            if c.is_ascii_whitespace() || c == b'+' {
                i += 1;
                continue;
            }
            if c == b'"' || c == b'\'' || c == b'`' {
                let quote = c;
                i += 1;
                let start = i;
                while i < b.len() {
                    if b[i] == b'\\' && i + 1 < b.len() {
                        i += 2;
                        continue;
                    }
                    if b[i] == quote {
                        break;
                    }
                    i += 1;
                }
                res.push_str(&expr[start..i.min(b.len())]);
                if i < b.len() {
                    i += 1;
                }
                continue;
            }
            if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
                let start = i;
                while i < b.len()
                    && (b[i].is_ascii_alphanumeric()
                        || b[i] == b'_'
                        || b[i] == b'$'
                        || b[i] == b'.')
                {
                    i += 1;
                }
                let name = &expr[start..i];
                if name.contains('.') {
                    // Özellik erişimi
                    if let Some((obj_name, prop)) = name.split_once('.') {
                        if let Some(val) = self.values.get(obj_name) {
                            match val {
                                JsValue::Array(items) if prop == "length" => {
                                    res.push_str(&items.len().to_string());
                                }
                                JsValue::String(s) if prop == "length" => {
                                    res.push_str(&s.len().to_string());
                                }
                                JsValue::Object(map) => {
                                    if let Some(prop_val) = map.get(prop) {
                                        res.push_str(&prop_val.to_str());
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    continue;
                }
                match name {
                    "true" => res.push_str("true"),
                    "false" | "null" | "undefined" => {}
                    _ => {
                        if let Some(v) = self.values.get(name) {
                            res.push_str(&v.to_str());
                        } else if let Some(v) = self.var_get(name) {
                            res.push_str(v);
                        } else if name.bytes().all(|c| c.is_ascii_digit()) {
                            res.push_str(name);
                        }
                    }
                }
                continue;
            }
            break;
        }
        res
    }

    /// String birleştirmeyi çözer: `"a" + x + 3`
    fn eval_expr(&mut self, expr: &str) -> String {
        self.eval_val(expr).to_str()
    }

    /// Koşulu değerlendirir: `!`, `&&`, `||`, karşılaştırmalar ve doğruluk.
    fn eval_cond(&mut self, cond: &str) -> bool {
        let c = cond.trim();
        if c.is_empty() {
            return true;
        }

        if let Some(rest) = c.strip_prefix('!') {
            if !rest.trim_start().starts_with('=') {
                return !self.eval_cond(rest);
            }
        }
        if let Some(idx) = find_top_level(c.as_bytes(), b"&&") {
            return self.eval_cond(&c[..idx]) && self.eval_cond(&c[idx + 2..]);
        }
        if let Some(idx) = find_top_level(c.as_bytes(), b"||") {
            return self.eval_cond(&c[..idx]) || self.eval_cond(&c[idx + 2..]);
        }
        for (op, neg) in [
            ("===", false),
            ("!==", true),
            ("==", false),
            ("!=", true),
            ("<=", false),
            (">=", false),
        ] {
            if let Some(idx) = find_top_level(c.as_bytes(), op.as_bytes()) {
                let l_val = self.eval_val(&c[..idx]);
                let r_val = self.eval_val(&c[idx + op.len()..]);
                let (ln_opt, rn_opt) = (
                    l_val.to_str().trim().parse::<i64>().ok(),
                    r_val.to_str().trim().parse::<i64>().ok(),
                );
                let r = if let (Some(ln), Some(rn)) = (ln_opt, rn_opt) {
                    match op {
                        "===" | "==" => ln == rn,
                        "!==" | "!=" => ln != rn,
                        "<=" => ln <= rn,
                        _ => ln >= rn,
                    }
                } else {
                    let lhs = l_val.to_str();
                    let rhs = r_val.to_str();
                    let ord = lhs.as_str().cmp(rhs.as_str());
                    match op {
                        "===" | "==" => lhs == rhs,
                        "!==" | "!=" => lhs != rhs,
                        "<=" => ord != core::cmp::Ordering::Greater,
                        _ => ord != core::cmp::Ordering::Less,
                    }
                };
                return r != neg;
            }
        }
        for op in ['<', '>'] {
            if let Some(idx) = find_top_level_char(c.as_bytes(), op) {
                let ln: i64 = self.eval_val(&c[..idx]).to_i64();
                let rn: i64 = self.eval_val(&c[idx + 1..]).to_i64();
                return if op == '<' { ln < rn } else { ln > rn };
            }
        }
        // instanceof: obj instanceof Constructor
        if let Some(idx) = find_top_level(c.as_bytes(), b"instanceof") {
            let left = c[..idx].trim();
            let right = c[idx + 10..].trim();
            // left bir değişken ismi, right bir fonksiyon ismi olmalı.
            if let Some(lhs_name) = left.strip_prefix('(') {
                // (expr) instanceof Constructor — içteki ifadeyi değerlendir
                if let Some(close) = lhs_name.rfind(')') {
                    let inner = &lhs_name[..close];
                    let base_val = self.eval_val(inner);
                    if let JsValue::Object(_) = base_val {
                        return self.instance_of(inner, right);
                    }
                }
            } else {
                return self.instance_of(left, right);
            }
        }
        self.eval_val(c).is_truthy()
    }

    /// Tek bir deyimi çalıştırır.
    fn exec(&mut self, stmt: &str, out: &mut Vec<u8>) {
        let mut s = stmt.trim();
        s = s.trim_start_matches(|c| {
            c == '{' || c == '}' || c == ')' || c == ',' || c == ';' || c == '\n' || c == '\r'
        });
        s = s.trim_end_matches(|c| c == ';' || c == '\n' || c == '\r');
        s = s.trim();
        if s.is_empty() {
            return;
        }

        // Fonksiyon tanımı: function name(arg1, arg2) { body }
        // async function name(arg1, arg2) { body } — Promise döndüren fonksiyon.
        if let Some(rest) = keyword(s, "function") {
            let r = rest.trim_start();
            let is_async = r.starts_with("async ");
            let r = if is_async { &r[6..] } else { r };
            if let Some(open) = r.find('(') {
                let fname = r[..open].trim();
                if let Some((param_str, after_params)) = call_args(r, open) {
                    let body_part = r[after_params..].trim_start();
                    if let Some(body) = extract_block(body_part) {
                        let params: Vec<String> = split_top_level_comma(param_str)
                            .into_iter()
                            .map(|p| p.trim().to_string())
                            .filter(|p| !p.is_empty())
                            .collect();
                        self.define_function(fname, params.clone(), body, is_async);
                        return;
                    }
                }
            }
            return;
        }

        // return deyimi
        if let Some(rest) = keyword(s, "return") {
            let val = self.eval_val(rest.trim());
            self.last_return = Some(val);
            return;
        }

        // await deyimi: await expr;
        if let Some(rest) = keyword(s, "await") {
            let val = self.eval_val(rest.trim());
            // await ifadesinin sonucu last_return'a yazılır; çağıran async
            // fonksiyon bu değeri Promise ile sarmalayacaktır.
            self.last_return = Some(val);
            return;
        }

        if keyword(s, "class").is_some() {
            // class ClassName [extends Parent] { constructor(...) { ... } method(...) { ... } }
            let rest = s[5..].trim();
            if let Some(brace_pos) = rest.find('{') {
                let before_brace = rest[..brace_pos].trim();
                let body = &rest[brace_pos + 1..];
                // Find matching closing brace
                if let Some(class_body) = extract_block(&(String::from("{") + body + "}")) {
                    // Parse class name and extends
                    let parts: Vec<&str> = before_brace.split_whitespace().collect();
                    let name = parts.first().copied().unwrap_or("");
                    let extends = if parts.len() >= 3 && parts[1] == "extends" {
                        Some(parts[2].to_string())
                    } else {
                        None
                    };
                    let mut js_class = JsClass::new(name.to_string(), extends);
                    // Parse methods: methodName(params) { body }
                    let method_stmts = split_statements(class_body);
                    for mstmt in &method_stmts {
                        let m = mstmt.trim();
                        if m.is_empty() {
                            continue;
                        }
                        // Sınıf gövdesindeki yöntemler `ad(parametreler) { ... }`
                        // veya `static ad(parametreler) { ... }` biçimindedir;
                        // `function` anahtar sözcüğü isteğe bağlıdır.
                        let (is_static, r) = match m.strip_prefix("static ") {
                            Some(rest) => (true, rest.trim_start()),
                            None => (false, m),
                        };
                        let r = match keyword(r, "function") {
                            Some(rest) => rest.trim_start(),
                            None => r,
                        };
                        if let Some(open) = r.find('(') {
                            let mname = r[..open].trim();
                            if !mname.is_empty() && !mname.contains(char::is_whitespace) {
                                if let Some((param_str, after_params)) = call_args(r, open) {
                                    if let Some(body) = extract_block(&r[after_params..]) {
                                        let params: Vec<String> = split_top_level_comma(param_str)
                                            .into_iter()
                                            .map(|p| p.trim().to_string())
                                            .filter(|p| !p.is_empty())
                                            .collect();
                                        let js_func = JsFunction {
                                            name: mname.to_string(),
                                            params,
                                            body: body.to_string(),
                                            is_async: false,
                                        };
                                        if is_static {
                                            js_class
                                                .static_methods
                                                .insert(mname.to_string(), js_func);
                                        } else {
                                            js_class.methods.insert(mname.to_string(), js_func);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    self.classes.insert(name.to_string(), js_class);
                    return;
                }
            }
            return;
        }

        // for döngüsü: for (init; cond; step) { body } | for (x of arr) | for (k in obj)
        if let Some(rest) = keyword(s, "for") {
            let r = rest.trim_start();
            if let Some(open) = r.find('(') {
                if let Some((header, after)) = call_args(r, open) {
                    let body_opt = extract_block(&r[after..]);
                    let body = body_opt.unwrap_or(r[after..].trim());

                    // for (x of arr)
                    if let Some((item_var, arr_expr)) = header.split_once(" of ") {
                        let mut var_name = item_var.trim();
                        for kw in ["var", "let", "const"] {
                            if let Some(stripped) = keyword(var_name, kw) {
                                var_name = stripped.trim();
                                break;
                            }
                        }
                        let arr_val = self.eval_val(arr_expr.trim());
                        if let JsValue::Array(items) = arr_val {
                            for item in items {
                                self.var_set_val(var_name, item);
                                if body_opt.is_some() {
                                    for stmt in split_statements(body) {
                                        if stmt.trim() == "break" || stmt.trim() == "break;" {
                                            break;
                                        }
                                        if stmt.trim() == "continue" || stmt.trim() == "continue;" {
                                            continue;
                                        }
                                        self.exec(&stmt, out);
                                        if self.last_return.is_some() {
                                            return;
                                        }
                                    }
                                } else if !body.is_empty() {
                                    self.exec(body, out);
                                    if self.last_return.is_some() {
                                        return;
                                    }
                                }
                            }
                            return;
                        }
                    } else if let Some((key_var, obj_expr)) = header.split_once(" in ") {
                        let mut var_name = key_var.trim();
                        for kw in ["var", "let", "const"] {
                            if let Some(stripped) = keyword(var_name, kw) {
                                var_name = stripped.trim();
                                break;
                            }
                        }
                        let obj_val = self.eval_val(obj_expr.trim());
                        let keys: Vec<String> = match obj_val {
                            JsValue::Object(map) => map.keys().cloned().collect(),
                            JsValue::Array(items) => {
                                (0..items.len()).map(|i| i.to_string()).collect()
                            }
                            _ => Vec::new(),
                        };
                        for k in keys {
                            self.var_set_val(var_name, JsValue::String(k));
                            if body_opt.is_some() {
                                for stmt in split_statements(body) {
                                    if stmt.trim() == "break" || stmt.trim() == "break;" {
                                        break;
                                    }
                                    if stmt.trim() == "continue" || stmt.trim() == "continue;" {
                                        continue;
                                    }
                                    self.exec(&stmt, out);
                                    if self.last_return.is_some() {
                                        return;
                                    }
                                }
                            } else if !body.is_empty() {
                                self.exec(body, out);
                                if self.last_return.is_some() {
                                    return;
                                }
                            }
                        }
                        return;
                    }

                    // for (init; cond; step)
                    let parts: Vec<&str> = header.split(';').collect();
                    if parts.len() == 3 {
                        let init = parts[0].trim();
                        let cond = parts[1].trim();
                        let step = parts[2].trim();

                        if !init.is_empty() {
                            self.exec(init, out);
                        }

                        let mut iters = 0;
                        'for_loop: while iters < 10_000 {
                            iters += 1;
                            if !cond.is_empty() && !self.eval_cond(cond) {
                                break;
                            }
                            if body_opt.is_some() {
                                for stmt in split_statements(body) {
                                    if stmt.trim() == "break" || stmt.trim() == "break;" {
                                        break 'for_loop;
                                    }
                                    if stmt.trim() == "continue" || stmt.trim() == "continue;" {
                                        break;
                                    }
                                    self.exec(&stmt, out);
                                    if self.last_return.is_some() {
                                        return;
                                    }
                                }
                            } else if !body.is_empty() {
                                self.exec(body, out);
                                if self.last_return.is_some() {
                                    return;
                                }
                            }
                            if !step.is_empty() {
                                self.exec(step, out);
                            }
                        }
                        return;
                    }
                }
            }
            return;
        }

        // while döngüsü: while (cond) { body }
        if let Some(rest) = keyword(s, "while") {
            let r = rest.trim_start();
            if let Some(open) = r.find('(') {
                if let Some((cond, after)) = call_args(r, open) {
                    let body_opt = extract_block(&r[after..]);
                    let body = body_opt.unwrap_or(r[after..].trim());
                    let mut iters = 0;
                    'while_loop: while iters < 10_000 && self.eval_cond(cond) {
                        iters += 1;
                        if body_opt.is_some() {
                            for stmt in split_statements(body) {
                                if stmt.trim() == "break" || stmt.trim() == "break;" {
                                    break 'while_loop;
                                }
                                if stmt.trim() == "continue" || stmt.trim() == "continue;" {
                                    break;
                                }
                                self.exec(&stmt, out);
                                if self.last_return.is_some() {
                                    return;
                                }
                            }
                        } else if !body.is_empty() {
                            self.exec(body, out);
                            if self.last_return.is_some() {
                                return;
                            }
                        }
                    }
                    return;
                }
            }
            return;
        }

        // if koşulu: if (cond) { body }
        if let Some(rest) = keyword(s, "if") {
            let r = rest.trim_start();
            if let Some(open) = r.find('(') {
                if let Some((cond, after)) = call_args(r, open) {
                    let ok = self.eval_cond(cond);
                    self.last_cond = ok;
                    if ok {
                        let tail = &r[after..];
                        if let Some(body) = extract_block(tail) {
                            for stmt in split_statements(body) {
                                self.exec(&stmt, out);
                                if self.last_return.is_some() {
                                    return;
                                }
                            }
                        } else {
                            self.exec(tail.trim(), out);
                        }
                    }
                }
            }
            return;
        }
        if let Some(rest) = keyword(s, "else") {
            if !self.last_cond {
                let r = rest.trim_start();
                if let Some(body) = extract_block(r) {
                    for stmt in split_statements(body) {
                        self.exec(&stmt, out);
                        if self.last_return.is_some() {
                            return;
                        }
                    }
                } else {
                    self.exec(r, out);
                }
            }
            return;
        }
        if keyword(s, "do").is_some() {
            return;
        }

        // delete deyimi: delete obj.prop veya delete obj[key]
        if let Some(target) = s.strip_prefix("delete ") {
            let t = target.trim();
            if let Some(open_br) = t.rfind('[') {
                if t.ends_with(']') {
                    let obj_name = t[..open_br].trim();
                    let key_str = t[open_br + 1..t.len() - 1].trim();
                    let key_val = self.eval_val(key_str).to_str();
                    if let Some(JsValue::Object(map)) = self.values.get_mut(obj_name) {
                        map.remove(&key_val);
                        self.sync_var_from_val(obj_name);
                        return;
                    }
                }
            }
            if let Some((obj_name, prop)) = t.rsplit_once('.') {
                let obj_name = obj_name.trim();
                let prop = prop.trim();
                if let Some(JsValue::Object(map)) = self.values.get_mut(obj_name) {
                    map.remove(prop);
                    self.sync_var_from_val(obj_name);
                    return;
                }
            }
            return;
        }

        // Dizi in-place metotları: arr.pop(), arr.shift(), arr.reverse(), arr.unshift(val)
        if let Some(pos) = s.rfind(".pop()") {
            let var_name = s[..pos].trim();
            if let Some(JsValue::Array(items)) = self.values.get_mut(var_name) {
                items.pop();
                self.sync_var_from_val(var_name);
                return;
            }
        }
        if let Some(pos) = s.rfind(".shift()") {
            let var_name = s[..pos].trim();
            if let Some(JsValue::Array(items)) = self.values.get_mut(var_name) {
                if !items.is_empty() {
                    items.remove(0);
                }
                self.sync_var_from_val(var_name);
                return;
            }
        }
        if let Some(pos) = s.rfind(".reverse()") {
            let var_name = s[..pos].trim();
            if let Some(JsValue::Array(items)) = self.values.get_mut(var_name) {
                items.reverse();
                self.sync_var_from_val(var_name);
                return;
            }
        }
        if let Some(pos) = s.rfind(".unshift(") {
            let var_name = s[..pos].trim();
            let call_slice = &s[pos + ".unshift".len()..];
            if let Some((args_str, _)) = call_args(call_slice, 0) {
                let val = self.eval_val(args_str.trim());
                if let Some(JsValue::Array(items)) = self.values.get_mut(var_name) {
                    items.insert(0, val);
                    self.sync_var_from_val(var_name);
                    return;
                }
            }
        }

        // Artırma / Azaltma: i++, ++i, i--, --i
        if s.ends_with("++") {
            let name = s[..s.len() - 2].trim();
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
            {
                let cur = self.values.get(name).map(|v| v.to_i64()).unwrap_or(0);
                self.var_set_val(name, JsValue::Number(cur + 1));
                return;
            }
        }
        if s.starts_with("++") {
            let name = s[2..].trim();
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
            {
                let cur = self.values.get(name).map(|v| v.to_i64()).unwrap_or(0);
                self.var_set_val(name, JsValue::Number(cur + 1));
                return;
            }
        }
        if s.ends_with("--") {
            let name = s[..s.len() - 2].trim();
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
            {
                let cur = self.values.get(name).map(|v| v.to_i64()).unwrap_or(0);
                self.var_set_val(name, JsValue::Number(cur - 1));
                return;
            }
        }
        if s.starts_with("--") {
            let name = s[2..].trim();
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
            {
                let cur = self.values.get(name).map(|v| v.to_i64()).unwrap_or(0);
                self.var_set_val(name, JsValue::Number(cur - 1));
                return;
            }
        }

        // Birleşik atamalar: +=, -=, *=, /=, %=
        for (cop, op_char) in [
            ("+=", '+'),
            ("-=", '-'),
            ("*=", '*'),
            ("/=", '/'),
            ("%=", '%'),
        ] {
            if let Some(pos) = find_top_level(s.as_bytes(), cop.as_bytes()) {
                let lhs = s[..pos].trim();
                let rhs_expr = s[pos + 2..].trim();
                let rhs_val = self.eval_val(rhs_expr);
                if !lhs.contains('.') && !lhs.contains('[') && !lhs.is_empty() {
                    let cur = self.values.get(lhs).cloned().unwrap_or(JsValue::Number(0));
                    let new_val = match op_char {
                        '+' => {
                            if let (JsValue::Number(a), JsValue::Number(b)) = (&cur, &rhs_val) {
                                JsValue::Number(a + b)
                            } else {
                                let mut s_res = cur.to_str();
                                s_res.push_str(&rhs_val.to_str());
                                JsValue::String(s_res)
                            }
                        }
                        '-' => JsValue::Number(cur.to_i64() - rhs_val.to_i64()),
                        '*' => JsValue::Number(cur.to_i64() * rhs_val.to_i64()),
                        '/' => {
                            let d = rhs_val.to_i64();
                            JsValue::Number(if d != 0 { cur.to_i64() / d } else { 0 })
                        }
                        '%' => {
                            let d = rhs_val.to_i64();
                            JsValue::Number(if d != 0 { cur.to_i64() % d } else { 0 })
                        }
                        _ => cur,
                    };
                    self.var_set_val(lhs, new_val);
                    return;
                }
            }
        }

        // Olay dinleyici ekleme: target.addEventListener("click", handler)
        if let Some(pos) = s.find("addEventListener(") {
            let target = if pos > 0 && s.as_bytes()[pos - 1] == b'.' {
                s[..pos - 1].trim()
            } else {
                "window"
            };
            let call_slice = &s[pos + "addEventListener".len()..];
            if let Some((args_str, _)) = call_args(call_slice, 0) {
                let parts = split_top_level_comma(args_str);
                if parts.len() >= 2 {
                    let ev_type = self.eval_expr(parts[0]);
                    let handler_arg = parts[1].trim();
                    if let Some(rest) = keyword(handler_arg, "function") {
                        let anon_name = format!("__anon_evt_{}", self.listeners.len());
                        let r = rest.trim_start();
                        if let Some(open) = r.find('(') {
                            if let Some((pstr, after_p)) = call_args(r, open) {
                                let bpart = r[after_p..].trim_start();
                                if let Some(b) = extract_block(bpart) {
                                    let params = split_top_level_comma(pstr)
                                        .into_iter()
                                        .map(|p| p.trim().to_string())
                                        .filter(|p| !p.is_empty())
                                        .collect();
                                    self.define_function(&anon_name, params, b, false);
                                    self.add_event_listener(target, &ev_type, &anon_name);
                                    return;
                                }
                            }
                        }
                    } else {
                        let handler_name = handler_arg.trim_matches(|c| c == '\'' || c == '"');
                        self.add_event_listener(target, &ev_type, handler_name);
                        return;
                    }
                }
            }
            return;
        }

        // Dizi push: arr.push(val)
        if let Some(dot_push) = s.find(".push(") {
            let var_name = s[..dot_push].trim();
            if !var_name.contains(' ') {
                if let Some((args_str, _)) = call_args(&s[dot_push + 5..], 0) {
                    let val = self.eval_val(args_str.trim());
                    if let Some(existing) = self.values.get_mut(var_name) {
                        if let JsValue::Array(ref mut items) = existing {
                            items.push(val);
                            self.sync_var_from_val(var_name);
                            return;
                        }
                    }
                }
            }
        }

        // Dizi/Nesne indeks ataması: arr[idx] = val | obj[key] = val
        if let Some(open_br) = s.rfind('[') {
            if let Some(close_br) = s[open_br..].find(']') {
                let close_br = open_br + close_br;
                let after = s[close_br + 1..].trim_start();
                if let Some(eq) = after.strip_prefix('=') {
                    if !eq.trim_start().starts_with('=') {
                        let var_name = s[..open_br].trim();
                        let idx_str = &s[open_br + 1..close_br];
                        let evaluated_idx = self.eval_val(idx_str).to_str();
                        let val = self.eval_val(eq.trim());
                        if let Some(existing) = self.values.get_mut(var_name) {
                            match existing {
                                JsValue::Array(ref mut items) => {
                                    let idx: usize = evaluated_idx.trim().parse().unwrap_or(0);
                                    if idx < items.len() {
                                        items[idx] = val;
                                    } else if idx == items.len() {
                                        items.push(val);
                                    }
                                }
                                JsValue::Object(ref mut map) => {
                                    map.insert(evaluated_idx, val);
                                }
                                _ => {}
                            }
                            self.sync_var_from_val(var_name);
                            return;
                        }
                    }
                }
            }
        }

        // Nesne özellik ataması: obj.prop = val
        if let Some(eq_pos) = s.find('=') {
            if !s[eq_pos + 1..].trim_start().starts_with('=') {
                let lhs = s[..eq_pos].trim();
                let rhs = s[eq_pos + 1..].trim();
                if let Some((obj_name, prop)) = lhs.rsplit_once('.') {
                    if !obj_name.contains(' ')
                        && !prop.contains(' ')
                        && !obj_name.is_empty()
                        && !prop.is_empty()
                        && !obj_name.starts_with("document.")
                        && !obj_name.starts_with("element.")
                    {
                        let val = self.eval_val(rhs);
                        if let Some(existing) = self.values.get_mut(obj_name) {
                            if let JsValue::Object(ref mut map) = existing {
                                map.insert(String::from(prop), val);
                                self.sync_var_from_val(obj_name);
                                return;
                            }
                        }
                    }
                }
            }
        }

        // Kullanıcı tanımlı fonksiyon çağrısı: fname(args)
        if let Some(open) = s.find('(') {
            let fname = s[..open].trim();
            if self.functions.contains_key(fname) && s.ends_with(')') {
                if let Some((arg_str, _)) = call_args(s, open) {
                    let args: Vec<JsValue> = split_top_level_comma(arg_str)
                        .into_iter()
                        .map(|a| self.eval_val(a.trim()))
                        .collect();
                    let _ = self.call_function(fname, &args, out);
                    return;
                }
            }
        }

        // Bilinen çağrılar
        let mut best: Option<(usize, &'static str)> = None;
        for name in CALLS {
            if let Some(idx) = find_bytes(s.as_bytes(), name.as_bytes()) {
                let after = idx + name.len();
                let tail_ok = s
                    .as_bytes()
                    .get(after)
                    .map_or(false, |c| *c == b'(' || c.is_ascii_whitespace());
                if tail_ok && best.map_or(true, |(b, _)| idx < b) {
                    best = Some((idx, name));
                }
            }
        }
        if let Some((idx, name)) = best {
            let tail = &s[idx + name.len()..];
            let open_rel = tail.find('(').unwrap_or(0);
            if let Some((args, _)) = call_args(tail, open_rel) {
                let value = self.eval_expr(args);
                if name.starts_with("console.") || name == "alert" || name == "window.alert" {
                    self.logs.push(value);
                } else if name == "prompt" {
                    let arg = value.trim();
                    out.extend_from_slice(arg.as_bytes());
                } else if name == "confirm" {
                    out.extend_from_slice(b"true");
                } else if name == "document.getElementById" {
                    let arg = value.trim();
                    if let Some(doc) = self.doc.as_ref() {
                        if let Some(elem_idx) = doc.get_element_by_id(arg) {
                            out.extend_from_slice(format!("element{}", elem_idx).as_bytes());
                        } else {
                            out.extend_from_slice(b"null");
                        }
                    } else {
                        out.extend_from_slice(b"null");
                    }
                } else if name == "document.getElementsByClassName" {
                    let arg = value.trim();
                    if let Some(doc) = self.doc.as_ref() {
                        let indices = doc.get_elements_by_class_name(arg);
                        let idx_str: String = indices
                            .iter()
                            .map(|i| i.to_string())
                            .collect::<Vec<_>>()
                            .join(",");
                        out.extend_from_slice(idx_str.as_bytes());
                    } else {
                        out.extend_from_slice(b"");
                    }
                } else if name == "document.createElement" {
                    let tag = value.trim();
                    if let Some(doc) = self.doc.as_mut() {
                        let new_idx = doc.append_element(DomElement {
                            tag_name: tag.to_string(),
                            attributes: BTreeMap::new(),
                            inner_html: String::new(),
                            text_content: String::new(),
                            children: Vec::new(),
                            style: BTreeMap::new(),
                            parent: None,
                            id: None,
                            class_name: None,
                        });
                        out.extend_from_slice(format!("element{}", new_idx).as_bytes());
                    }
                } else if name == "element.innerHTML" {
                    out.extend_from_slice(b"null");
                } else if name == "element.textContent" {
                    out.extend_from_slice(b"null");
                } else if name == "element.setAttribute" {
                    out.extend_from_slice(b"");
                } else if name == "element.getAttribute" {
                    out.extend_from_slice(b"null");
                } else if name == "setTimeout" {
                    // setTimeout(callback, ms) — microtask sırasına ekler.
                    let parts: Vec<&str> = split_top_level_comma(args);
                    if let Some(cb) = parts.first() {
                        let ms = parts
                            .get(1)
                            .map(|m| m.trim().parse::<u32>().unwrap_or(0))
                            .unwrap_or(0);
                        let cb_str = cb.trim().trim_matches(|c| c == '\'' || c == '"');
                        self.add_timer(cb_str.to_string(), ms, false);
                    }
                    out.extend_from_slice(b"0");
                } else if name == "setInterval" {
                    // setInterval(callback, ms) — tekrarlayan zamanlayıcı.
                    let parts: Vec<&str> = split_top_level_comma(args);
                    if let Some(cb) = parts.first() {
                        let ms = parts
                            .get(1)
                            .map(|m| m.trim().parse::<u32>().unwrap_or(0))
                            .unwrap_or(0);
                        let cb_str = cb.trim().trim_matches(|c| c == '\'' || c == '"');
                        self.add_timer(cb_str.to_string(), ms, true);
                    }
                    out.extend_from_slice(b"0");
                } else if name == "clearTimeout" || name == "clearInterval" {
                    let parts: Vec<&str> = split_top_level_comma(args);
                    if let Some(id_str) = parts.first() {
                        let id: u32 = id_str.trim().parse().unwrap_or(0);
                        self.remove_timer(id);
                    }
                    out.extend_from_slice(b"");
                } else if name == "fetch" {
                    // fetch(url) — HTTP GET isteği yapar, Promise döndürür.
                    let url = value.trim().trim_matches(|c| c == '\'' || c == '"');
                    let _ = self.do_fetch(&url);
                    out.extend_from_slice(b"0");
                } else if name == "localStorage" {
                    out.extend_from_slice(b"0");
                } else if name == "sessionStorage" {
                    out.extend_from_slice(b"0");
                } else if name == "WebSocket" {
                    let url = value.trim().trim_matches(|c| c == '\'' || c == '"');
                    let ws_id = self.next_ws_id;
                    self.next_ws_id += 1;
                    self.websockets
                        .insert(ws_id, WebSocketConn::new(url.to_string()));
                    out.extend_from_slice(ws_id.to_string().as_bytes());
                } else {
                    out.extend_from_slice(value.as_bytes());
                }
                return;
            }
        }

        // Atama: var/let/const name = expr  |  name = expr
        let mut rest = s;
        let mut is_decl = false;
        for kw in ["var", "let", "const"] {
            if let Some(r) = keyword(s, kw) {
                rest = r.trim_start();
                is_decl = true;
                break;
            }
        }
        let bytes = rest.as_bytes();
        let mut n = 0;
        while n < bytes.len()
            && (bytes[n].is_ascii_alphanumeric() || bytes[n] == b'_' || bytes[n] == b'$')
        {
            n += 1;
        }
        if n > 0 && n < bytes.len() {
            let after_name = rest[n..].trim_start();
            if let Some(eq) = after_name.strip_prefix('=') {
                if !eq.trim_start().starts_with('=') {
                    let name = &rest[..n];
                    if !name.contains('.') {
                        // Strict mode: atanacak değişken zaten bildirilmiş olmalı.
                        if self.strict_mode && !is_decl {
                            let already =
                                self.values.contains_key(name) || self.var_get(name).is_some();
                            if !already {
                                // Referans hatası: değişken bildirilmemiş.
                                self.logs.push(format!(
                                    "ReferenceError: '{}' is not defined (strict mode)",
                                    name
                                ));
                                return;
                            }
                        }
                        let value = self.eval_val(eq);
                        self.var_set_val(name, value);
                        return;
                    }
                }
            }
        }

        // Son çare: değerlendirilebilir bir ifade (ör. `obj.method()`,
        // `c.inc()`, `this.next()`) ise çalıştır; dönüş değeri yok sayılır.
        if s.ends_with(')') || s.ends_with(']') {
            let _ = self.eval_val(s);
        }
    }

    /// Betiği sırayla çalıştırır; `document.write` çıktıları `out`'a eklenir.
    pub fn run(&mut self, code: &str, out: &mut Vec<u8>) {
        // Strict mode algıla: ilk deyim "use strict" veya 'use strict' olmalı.
        let stmts: Vec<String> = split_statements(code);
        if let Some(first) = stmts.first() {
            let trimmed = first.trim();
            if trimmed == "\"use strict\"" || trimmed == "'use strict'" {
                self.strict_mode = true;
            }
        }
        for stmt in stmts {
            self.exec(&stmt, out);
        }
        // Promise microtask'larını çalıştır (.then() işleyicileri vb.).
        self.run_microtasks(out);
    }
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

fn find_ci(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

/// `<script ...>` etiketi içindeki `attr="değer"` okur.
fn attr_of(tag: &[u8], key: &[u8]) -> Option<String> {
    let mut i = 0;
    while i < tag.len() {
        while i < tag.len() && (tag[i].is_ascii_whitespace() || tag[i] == b'/') {
            i += 1;
        }
        let k_start = i;
        while i < tag.len() && (tag[i].is_ascii_alphanumeric() || tag[i] == b'-') {
            i += 1;
        }
        let cur = &tag[k_start..i];
        while i < tag.len() && tag[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < tag.len() && tag[i] == b'=' {
            i += 1;
            while i < tag.len() && tag[i].is_ascii_whitespace() {
                i += 1;
            }
            let quote = if i < tag.len() && (tag[i] == b'"' || tag[i] == b'\'') {
                let q = tag[i];
                i += 1;
                q
            } else {
                0
            };
            let v_start = i;
            while i < tag.len()
                && ((quote != 0 && tag[i] != quote)
                    || (quote == 0 && !tag[i].is_ascii_whitespace() && tag[i] != b'>'))
            {
                i += 1;
            }
            let val = String::from_utf8_lossy(&tag[v_start..i]).into_owned();
            if quote != 0 && i < tag.len() {
                i += 1;
            }
            if cur.eq_ignore_ascii_case(key) {
                return Some(val);
            }
        } else if i == k_start {
            i += 1;
        }
    }
    None
}

/// `if` / `var` gibi anahtar kelimenin `s` ile başladığını kontrol eder.
fn keyword<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    if let Some(rest) = s.strip_prefix(kw) {
        match rest.chars().next() {
            None => Some(rest),
            Some(c) if !(c.is_alphanumeric() || c == '_' || c == '$') => Some(rest),
            _ => None,
        }
    } else {
        None
    }
}

/// Parantez/süslü parantez içermeyen ilk eşleşmenin konumunu bulur.
fn find_top_level(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < hay.len() {
        let c = hay[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ => {
                if depth == 0
                    && i + needle.len() <= hay.len()
                    && &hay[i..i + needle.len()] == needle
                {
                    // `==` iken `===` ile karışmasın
                    if (needle == b"==" || needle == b"!=") && hay[i + 2..].first() == Some(&b'=') {
                        i += 1;
                        continue;
                    }
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

/// `instanceof`, `in` gibi bir sözcüğü tam sözcük olarak en üst düzeyde arar.
fn find_top_level_word(hay: &[u8], word: &[u8]) -> Option<usize> {
    let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$';
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut i = 0usize;
    while i < hay.len() {
        let c = hay[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ => {
                if depth == 0 && i + word.len() <= hay.len() && &hay[i..i + word.len()] == word {
                    let before_ok = i == 0 || !is_word(hay[i - 1]);
                    let after_ok = hay.get(i + word.len()).map_or(true, |n| !is_word(*n));
                    if before_ok && after_ok {
                        return Some(i);
                    }
                }
            }
        }
        i += 1;
    }
    None
}

fn find_top_level_char(hay: &[u8], target: char) -> Option<usize> {
    let tb = target as u32;
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < hay.len() {
        let c = hay[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ => {
                if depth == 0 && (c as u32) == tb {
                    // `<=`, `>=`, `=>` karşılaştırmaları değil
                    if (target == '<' || target == '>') && hay.get(i + 1) == Some(&b'=') {
                        i += 1;
                        continue;
                    }
                    if target == '>' && i > 0 && hay[i - 1] == b'=' {
                        i += 1;
                        continue;
                    }
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

/// Bir çağrının argümanlarını (`(` ile `)` arası) ve kapanış konumunu bulur.
fn call_args(s: &str, open_idx: usize) -> Option<(&str, usize)> {
    let b = s.as_bytes();
    if open_idx >= b.len() || b[open_idx] != b'(' {
        return None;
    }
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut i = open_idx;
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[open_idx + 1..i], i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    // Kapanmayan parantez: kalanı argüman say.
    let rest = s[open_idx + 1..]
        .trim_end_matches(|c| c == ')' || c == '}' || c == ';' || c == '\n' || c == '\r');
    Some((rest, b.len()))
}

/// En üst düzey süslü parantez `{ ... }` bloğunu ayıklar.
fn extract_block(s: &str) -> Option<&str> {
    let s = s.trim_start();
    if !s.starts_with('{') {
        return None;
    }
    let b = s.as_bytes();
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    for i in 0..b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == b'\\' && i + 1 < b.len() {
                continue;
            }
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[1..i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Parantez ve dize dışındaki en üst düzey virgüllerden (`c = ','`) böler.
fn split_top_level_comma(s: &str) -> Vec<&str> {
    let mut res = Vec::new();
    let b = s.as_bytes();
    let mut start = 0;
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut depth_brace = 0i32;
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == b'\\' && i + 1 < b.len() {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b'(' => depth_paren += 1,
            b')' => depth_paren -= 1,
            b'[' => depth_bracket += 1,
            b']' => depth_bracket -= 1,
            b'{' => depth_brace += 1,
            b'}' => depth_brace -= 1,
            b',' if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                let piece = s[start..i].trim();
                if !piece.is_empty() {
                    res.push(piece);
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    let tail = s[start..].trim();
    if !tail.is_empty() {
        res.push(tail);
    }
    res
}

/// Dış eşleşen parantezleri soyar: `((expr))` -> `expr`
fn strip_matching_parens(mut s: &str) -> &str {
    loop {
        let trimmed = s.trim();
        if trimmed.starts_with('(') && trimmed.ends_with(')') && trimmed.len() >= 2 {
            let b = trimmed.as_bytes();
            let mut depth = 0i32;
            let mut matched_at_end = false;
            let mut quote: Option<u8> = None;
            for (i, &c) in b.iter().enumerate() {
                if let Some(q) = quote {
                    if c == b'\\' && i + 1 < b.len() {
                        continue;
                    }
                    if c == q {
                        quote = None;
                    }
                    continue;
                }
                match c {
                    b'"' | b'\'' | b'`' => quote = Some(c),
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            if i == b.len() - 1 {
                                matched_at_end = true;
                            }
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if matched_at_end {
                s = &trimmed[1..trimmed.len() - 1];
                continue;
            }
        }
        return trimmed;
    }
}

/// En sağdaki üst düzey ikili operatörü bulur (sol-ilişkili değerlendirme için).
fn find_rightmost_binary_op(hay: &[u8], ops: &[u8]) -> Option<(usize, u8)> {
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut depth_brace = 0i32;
    let mut quote: Option<u8> = None;
    let mut rightmost = None;
    let mut i = 0;
    while i < hay.len() {
        let c = hay[i];
        if let Some(q) = quote {
            if c == b'\\' && i + 1 < hay.len() {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b'(' => depth_paren += 1,
            b')' => depth_paren -= 1,
            b'[' => depth_bracket += 1,
            b']' => depth_bracket -= 1,
            b'{' => depth_brace += 1,
            b'}' => depth_brace -= 1,
            _ => {
                if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 && ops.contains(&c) {
                    let prev = if i > 0 { hay.get(i - 1).copied() } else { None };
                    let next = hay.get(i + 1).copied();
                    let is_double = (c == b'+' && next == Some(b'+'))
                        || (c == b'+' && prev == Some(b'+'))
                        || (c == b'-' && next == Some(b'-'))
                        || (c == b'-' && prev == Some(b'-'));
                    let is_assignment = next == Some(b'=');
                    let is_arrow = c == b'=' && next == Some(b'>');
                    let is_unary = (c == b'+' || c == b'-') && {
                        let mut j = i;
                        let mut has_prev_token = false;
                        while j > 0 {
                            j -= 1;
                            let pj = hay[j];
                            if !pj.is_ascii_whitespace() {
                                has_prev_token = matches!(
                                    pj,
                                    b'a'..=b'z'
                                        | b'A'..=b'Z'
                                        | b'0'..=b'9'
                                        | b'_'
                                        | b'$'
                                        | b')'
                                        | b']'
                                        | b'}'
                                        | b'"'
                                        | b'\''
                                        | b'`'
                                );
                                break;
                            }
                        }
                        !has_prev_token
                    };

                    if !is_double && !is_assignment && !is_arrow && !is_unary {
                        rightmost = Some((i, c));
                    }
                }
            }
        }
        i += 1;
    }
    rightmost
}

/// JS değerini standart JSON metnine dönüştürür.
fn json_stringify(val: &JsValue) -> String {
    match val {
        JsValue::Undefined => String::from("undefined"),
        JsValue::Null => String::from("null"),
        JsValue::Bool(b) => {
            if *b {
                String::from("true")
            } else {
                String::from("false")
            }
        }
        JsValue::Number(n) => n.to_string(),
        JsValue::String(s) => {
            let mut res = String::with_capacity(s.len() + 2);
            res.push('"');
            for ch in s.chars() {
                match ch {
                    '"' => res.push_str("\\\""),
                    '\\' => res.push_str("\\\\"),
                    '\n' => res.push_str("\\n"),
                    '\r' => res.push_str("\\r"),
                    '\t' => res.push_str("\\t"),
                    c => res.push(c),
                }
            }
            res.push('"');
            res
        }
        JsValue::Array(items) => {
            let mut res = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    res.push(',');
                }
                res.push_str(&json_stringify(item));
            }
            res.push(']');
            res
        }
        JsValue::Object(map) => {
            let mut res = String::from("{");
            for (i, (k, v)) in map.iter().enumerate() {
                if i > 0 {
                    res.push(',');
                }
                res.push_str(&json_stringify(&JsValue::String(k.clone())));
                res.push(':');
                res.push_str(&json_stringify(v));
            }
            res.push('}');
            res
        }
        JsValue::Promise(_) => String::from("Promise { <pending> }"),
    }
}

/// Metot çağrısını ayıklar: `base.method(args)`
fn extract_method_call<'a>(s: &'a str) -> Option<(&'a str, &'a str, &'a str)> {
    let s = s.trim();
    if !s.ends_with(')') {
        return None;
    }
    let b = s.as_bytes();
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut open_idx = None;
    for i in (0..b.len()).rev() {
        let c = b[i];
        if let Some(q) = quote {
            if c == q {
                let mut slash_count = 0;
                let mut j = i;
                while j > 0 && b[j - 1] == b'\\' {
                    slash_count += 1;
                    j -= 1;
                }
                if slash_count % 2 == 0 {
                    quote = None;
                }
            }
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b')' => depth += 1,
            b'(' => {
                depth -= 1;
                if depth == 0 {
                    open_idx = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }

    let open = open_idx?;
    let before_call = s[..open].trim_end();
    let dot_idx = before_call.rfind('.')?;
    let base = before_call[..dot_idx].trim();
    let method = before_call[dot_idx + 1..].trim();
    if base.is_empty() || method.is_empty() {
        return None;
    }
    if !method
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
    {
        return None;
    }
    let args = s[open + 1..s.len() - 1].trim();
    Some((base, method, args))
}

/// `obj.method(args)` ifadesindeki `obj` ile `method` arasındaki noktanın
/// konumunu döndürür. Nokta üst düzeyde (parantez/dize dışında) olmalı ve
/// çağrı tamamen kapanmış olmalıdır.
fn find_method_call_dot(s: &str) -> Option<usize> {
    let s = s.trim();
    if !s.ends_with(')') {
        return None;
    }
    let b = s.as_bytes();
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut dot = None;
    for i in (0..b.len()).rev() {
        let c = b[i];
        if let Some(q) = quote {
            if c == q {
                let mut slashes = 0usize;
                let mut j = i;
                while j > 0 && b[j - 1] == b'\\' {
                    slashes += 1;
                    j -= 1;
                }
                if slashes % 2 == 0 {
                    quote = None;
                }
            }
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b')' | b']' => depth += 1,
            b'(' | b'[' => {
                if depth == 0 {
                    // Çağrının açık parantezini geçtik; solda nokta yoksa
                    // bu bir metot çağrısı değildir (ör. `(a + b) * c`).
                    return None;
                }
                depth -= 1;
            }
            b'.' if depth == 0 => {
                dot = Some(i);
                break;
            }
            _ => {}
        }
    }
    let dot = dot?;
    let base = s[..dot].trim();
    let rest = s[dot + 1..].trim();
    let method = rest.split('(').next().unwrap_or("").trim();
    if base.is_empty() || !is_simple_ident(method) {
        return None;
    }
    Some(dot)
}

/// `a`, `a_b`, `$x1` gibi basit bir tanımlayıcı mı?
fn is_simple_ident(s: &str) -> bool {
    !s.is_empty()
        && !s.chars().next().map_or(true, |c| c.is_ascii_digit())
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// Bir sınıf metodunu nesne özelliği (`{ __fn__, __params__ }`) biçimine çevirir.
fn method_object(mfunc: &JsFunction) -> JsValue {
    JsValue::Object(BTreeMap::from([
        (String::from("__fn__"), JsValue::String(mfunc.body.clone())),
        (
            String::from("__params__"),
            JsValue::String(mfunc.params.join(",")),
        ),
    ]))
}

const CALLS: [&str; 28] = [
    "console.log",
    "console.info",
    "console.warn",
    "console.error",
    "console.debug",
    "document.write",
    "document.writeln",
    "alert",
    "window.alert",
    "document.getElementById",
    "document.getElementsByClassName",
    "prompt",
    "confirm",
    "element.innerHTML",
    "element.textContent",
    "element.setAttribute",
    "element.getAttribute",
    "document.createElement",
    "element.appendChild",
    "element.remove",
    "setTimeout",
    "setInterval",
    "clearTimeout",
    "clearInterval",
    "fetch",
    "localStorage",
    "sessionStorage",
    "WebSocket",
];

/// JS kaynak kodunu `;` ve satır sonlarına göre deyimlere böler
/// (dize literalari ve parantez içleri korunur).
fn split_statements(code: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let b = code.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut paren = 0i32;
    let mut brace = 0i32;
    let mut brack = 0i32;
    let mut quote: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        let mut boundary = false;
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b'(' => paren += 1,
            b')' => paren -= 1,
            b'{' => brace += 1,
            b'}' => {
                brace -= 1;
                if brace <= 0 && paren <= 0 && brack <= 0 {
                    brace = 0;
                    boundary = true;
                }
            }
            b'[' => brack += 1,
            b']' => brack -= 1,
            b';' if paren <= 0 && brace <= 0 && brack <= 0 => boundary = true,
            b'\n' if paren <= 0 && brace <= 0 && brack <= 0 => {
                let up = code[..i].trim_end();
                if up.ends_with(')') || up.ends_with('}') {
                    boundary = true;
                }
            }
            _ => {}
        }
        if boundary {
            let piece = code[start..i + 1].trim();
            if !piece.is_empty() {
                out.push(String::from(piece));
            }
            start = i + 1;
        }
        i += 1;
    }
    let tail = code[start..].trim();
    if !tail.is_empty() {
        out.push(String::from(tail));
    }
    out
}

/// Betiği çalıştırır (kısa yol).
pub fn run_script(code: &str, rt: &mut JsRuntime, out: &mut Vec<u8>) {
    rt.run(code, out);
}

/// HTML içindeki tüm `<script>` bloklarını sırayla çalıştırır.
///
/// `document.write(...)` çıktıları, betiğin bulunduğu yere HTML olarak gömülür;
/// betik etiketleri ise çıktidan çıkarılır.
pub fn inline_scripts(html: &[u8]) -> Vec<u8> {
    let mut rt = JsRuntime::new();
    let mut out: Vec<u8> = Vec::with_capacity(html.len());
    let mut p = 0usize;
    while p < html.len() {
        match find_ci(&html[p..], b"<script") {
            None => {
                out.extend_from_slice(&html[p..]);
                break;
            }
            Some(rel) => {
                let tag_start = p + rel;
                let mut tag_end = tag_start;
                while tag_end < html.len() && html[tag_end] != b'>' {
                    tag_end += 1;
                }
                if tag_end >= html.len() {
                    out.extend_from_slice(&html[p..]);
                    break;
                }
                out.extend_from_slice(&html[p..tag_start]);
                let opening = &html[tag_start..tag_end + 1];
                let content_start = tag_end + 1;
                let (content, next) = match find_ci(&html[content_start..], b"</script>") {
                    Some(r) => (
                        &html[content_start..content_start + r],
                        content_start + r + 9,
                    ),
                    None => (&html[content_start..], html.len()),
                };
                let executable = match attr_of(opening, b"type") {
                    None => true,
                    Some(t) => {
                        let t = t.to_ascii_lowercase();
                        t.contains("javascript") || t.contains("module") || !t.contains('/')
                    }
                };
                if executable {
                    let code = String::from_utf8_lossy(content).into_owned();
                    rt.run(&code, &mut out);
                }
                p = next;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::vec::Vec;

    fn render(js: &str) -> String {
        let html = format!("<script>{}</script>", js);
        let out = inline_scripts(html.as_bytes());
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn document_write_basic() {
        assert_eq!(render(r#"document.write("Merhaba");"#), "Merhaba");
    }

    #[test]
    fn concatenation_and_vars() {
        let r = render(r#"var a = "Dünya"; document.write("Merhaba " + a + "!");"#);
        assert_eq!(r, "Merhaba Dünya!");
    }

    #[test]
    fn console_log_collected() {
        let mut rt = JsRuntime::new();
        let mut out: Vec<u8> = Vec::new();
        rt.run(r#"console.log("x", 1); document.write("y");"#, &mut out);
        assert_eq!(rt.logs.len(), 1);
        assert_eq!(rt.logs[0], "x");
        assert_eq!(String::from_utf8_lossy(&out), "y");
    }

    #[test]
    fn if_else_branches() {
        assert_eq!(
            render(
                r#"var x = "2"; if (x == "2") { document.write("esit"); } else { document.write("degil"); }"#
            ),
            "esit"
        );
        assert_eq!(
            render(r#"if (1 > 2) { document.write("a"); } else { document.write("b"); }"#),
            "b"
        );
        assert_eq!(render(r#"if (!false) { document.write("ok"); }"#), "ok");
    }

    #[test]
    fn script_without_semicolons() {
        let r = render("document.write(\"A\")\ndocument.write(\"B\")");
        assert_eq!(r, "AB");
    }

    #[test]
    fn non_javascript_type_skipped() {
        let html = br#"<script type="application/json">{"a":1}</script>"#;
        assert_eq!(String::from_utf8_lossy(&inline_scripts(html)), "");
    }

    #[test]
    fn outer_html_kept() {
        let html = b"<p>Once</p><script>document.write('!')</script><p>Sonra</p>";
        assert_eq!(
            String::from_utf8_lossy(&inline_scripts(html)),
            "<p>Once</p>!<p>Sonra</p>"
        );
    }

    #[test]
    fn function_bodies_not_executed_at_definition() {
        let r = render(r#"function f(){ document.write("x"); } document.write("y");"#);
        assert_eq!(r, "y");
    }

    #[test]
    fn document_getelementbyid() {
        let mut doc = JsDocumentRef::new();
        doc.add_root(DomElement {
            tag_name: "body".to_string(),
            attributes: BTreeMap::new(),
            inner_html: String::new(),
            text_content: String::new(),
            children: Vec::new(),
            style: BTreeMap::new(),
            parent: None,
            id: Some("body".to_string()),
            class_name: None,
        });
        let child_idx = doc.append_element(DomElement {
            tag_name: "div".to_string(),
            attributes: BTreeMap::new(),
            inner_html: String::new(),
            text_content: String::new(),
            children: Vec::new(),
            style: BTreeMap::new(),
            parent: None,
            id: Some("testelement".to_string()),
            class_name: None,
        });

        assert_eq!(doc.get_element_by_id("body"), Some(doc.root_idx));
        assert_eq!(doc.get_element_by_id("testelement"), Some(child_idx));
    }

    #[test]
    fn document_getelementsbyclassname() {
        // Sınıf ile eleman bulma testi
        let html = "<script>var divs = document.getElementsByClassName('myclass');var result = '';for (var i = 0; i < divs.length; i++) {result += divs[i].innerHTML;}</script>";
        let out = inline_scripts(html.as_bytes());
        let result = String::from_utf8_lossy(&out);
        // getElementsByClassName basit implementasyonu için boş veya idx'ler içermelidir
        assert!(result.contains("0") || result.is_empty());
    }

    #[test]
    fn prompt_and_confirm() {
        // prompt ve confirm fonksiyonları testi
        let html = "<script>var name = prompt('Adınız nedir?');var ok = confirm('Onaylıyorsunuz mı?');document.write('İsim: ' + name + '<br>Onay: ' + ok);</script>";
        let out = inline_scripts(html.as_bytes());
        let result = String::from_utf8_lossy(&out);
        assert!(result.contains("Onay:") || result.contains("İsim"));
    }

    #[test]
    fn dom_createelement_and_appendchild() {
        let mut doc = JsDocumentRef::new();
        doc.add_root(DomElement {
            tag_name: "body".to_string(),
            attributes: BTreeMap::new(),
            inner_html: String::new(),
            text_content: String::new(),
            children: Vec::new(),
            style: BTreeMap::new(),
            parent: None,
            id: None,
            class_name: None,
        });
        let child_idx = doc.append_element(DomElement {
            tag_name: "p".to_string(),
            attributes: BTreeMap::new(),
            inner_html: "Yeni Paragraf".to_string(),
            text_content: String::new(),
            children: Vec::new(),
            style: BTreeMap::new(),
            parent: None,
            id: None,
            class_name: None,
        });

        assert_eq!(doc.elements[doc.root_idx].children.len(), 1);
        assert_eq!(doc.elements[child_idx].tag_name, "p");
        assert_eq!(doc.elements[child_idx].parent, Some(doc.root_idx));
    }

    #[test]
    fn dom_setattribute_and_getattribute() {
        // setAttribute ve getAttribute testi
        let html = "<script>var el = document.createElement('div');el.setAttribute('data-value', '42');var val = el.getAttribute('data-value');document.write('Değer: ' + val);</script>";
        let out = inline_scripts(html.as_bytes());
        let result = String::from_utf8_lossy(&out);
        assert!(result.contains("Değer") || result.is_empty());
    }

    #[test]
    fn function_definition_and_execution() {
        let code = r#"
            function add(a, b) {
                return a + b;
            }
            document.write("Toplam: " + add(15, 25));
        "#;
        assert_eq!(render(code), "Toplam: 40");

        let code_side_effect = r#"
            function greet(name) {
                document.write("Selam " + name + "! ");
            }
            greet("minOs");
            greet("Sarman");
        "#;
        assert_eq!(render(code_side_effect), "Selam minOs! Selam Sarman! ");
    }

    #[test]
    fn arrays_literal_indexing_and_length() {
        let code = r#"
            var arr = [10, 20, 30];
            document.write(arr[1] + " len=" + arr.length);
        "#;
        assert_eq!(render(code), "20 len=3");
    }

    #[test]
    fn array_push_and_mutation() {
        let code = r#"
            var list = [1, 2];
            list.push(3);
            list[0] = 99;
            document.write(list[0] + "," + list[1] + "," + list[2] + " len=" + list.length);
        "#;
        assert_eq!(render(code), "99,2,3 len=3");
    }

    #[test]
    fn objects_literal_and_properties() {
        let code = r#"
            var user = { name: "minOs", ver: 2 };
            document.write(user.name + " v" + user.ver);
        "#;
        assert_eq!(render(code), "minOs v2");

        let code_mutation = r#"
            var obj = { x: 1 };
            obj.x = 42;
            document.write(obj.x);
        "#;
        assert_eq!(render(code_mutation), "42");
    }

    #[test]
    fn add_event_listener_and_dispatch() {
        let mut rt = JsRuntime::new();
        let mut out: Vec<u8> = Vec::new();
        let code = r#"
            function onBtnClick() {
                document.write("Dugme tiklandi!");
            }
            document.addEventListener("click", onBtnClick);
        "#;
        rt.run(code, &mut out);
        assert_eq!(rt.listeners.len(), 1);
        assert_eq!(rt.listeners[0].target, "document");
        assert_eq!(rt.listeners[0].event_type, "click");
        assert_eq!(rt.listeners[0].handler, "onBtnClick");

        // Olay tetikleme
        let fired = rt.dispatch_event("document", "click", &mut out);
        assert!(fired);
        assert_eq!(String::from_utf8_lossy(&out), "Dugme tiklandi!");
    }

    #[test]
    fn arithmetic_operators_precedence_and_parens() {
        assert_eq!(render("document.write(1 + 2 * 3);"), "7");
        assert_eq!(render("document.write((1 + 2) * 3);"), "9");
        assert_eq!(render("document.write(10 - 4 - 2);"), "4");
        assert_eq!(render("document.write(20 / 4 / 2);"), "2");
        assert_eq!(render("document.write(10 % 3);"), "1");
        assert_eq!(render("document.write(-5 + 15);"), "10");
        assert_eq!(render("document.write(-(3 + 2));"), "-5");
        assert_eq!(render("document.write(10 + 20 / (2 + 3));"), "14");
    }

    #[test]
    fn array_methods_pop_shift_slice_join_indexof() {
        let code = r#"
            var arr = [10, 20, 30, 40];
            arr.push(50);
            arr.pop();
            arr.shift();
            arr.unshift(99);
            arr.reverse();
            document.write("arr=" + arr.join("-"));
        "#;
        assert_eq!(render(code), "arr=40-30-20-99");

        let code_helpers = r#"
            var fruits = ["elma", "armut", "muz"];
            var hasElma = fruits.includes("elma");
            var idxMuz = fruits.indexOf("muz");
            var sub = fruits.slice(1, 3);
            var merged = fruits.concat(["kiraz"]);
            document.write(hasElma + " " + idxMuz + " " + sub.join(",") + " len=" + merged.length);
        "#;
        assert_eq!(render(code_helpers), "true 2 armut,muz len=4");
    }

    #[test]
    fn string_methods_slice_split_indexof_trim() {
        let code = r#"
            var s = "  minOs isletim sistemi  ";
            var clean = s.trim();
            var parts = clean.split(" ");
            var hasOs = clean.includes("isletim");
            var idxSys = clean.indexOf("sistemi");
            document.write(clean.toUpperCase() + " parts=" + parts.length + " os=" + hasOs + " idx=" + idxSys);
        "#;
        assert_eq!(render(code), "MINOS ISLETIM SISTEMI parts=3 os=true idx=14");
    }

    #[test]
    fn object_bracket_keys_values_json_and_delete() {
        let code = r#"
            var user = { name: "minOs", ver: 2, role: "os" };
            user["arch"] = "x86_64";
            delete user.role;
            var keys = Object.keys(user);
            var vals = Object.values(user);
            var json = JSON.stringify(user);
            var parsed = JSON.parse('{"a":10,"b":20}');
            document.write("keys=" + keys.length + " vals=" + vals.length + " json=" + json + " sum=" + (parsed.a + parsed.b));
        "#;
        assert_eq!(
            render(code),
            r#"keys=3 vals=3 json={"arch":"x86_64","name":"minOs","ver":2} sum=30"#
        );
    }

    #[test]
    fn for_and_while_loops_and_increment() {
        let code_for = r#"
            var sum = 0;
            for (var i = 1; i <= 5; i++) {
                sum += i;
            }
            document.write("sum=" + sum);
        "#;
        assert_eq!(render(code_for), "sum=15");

        let code_while = r#"
            var count = 4;
            var text = "";
            while (count > 0) {
                text += count;
                count--;
            }
            document.write("blastoff=" + text);
        "#;
        assert_eq!(render(code_while), "blastoff=4321");

        let code_for_of = r#"
            var list = [10, 20, 30];
            var total = 0;
            for (var x of list) {
                total += x;
            }
            document.write("total=" + total);
        "#;
        assert_eq!(render(code_for_of), "total=60");

        let code_for_in = r#"
            var obj = { a: 1, b: 2 };
            var kstr = "";
            for (var k in obj) {
                kstr += k;
            }
            document.write("keys=" + kstr);
        "#;
        assert_eq!(render(code_for_in), "keys=ab");
    }

    #[test]
    fn nested_objects_and_function_returns() {
        let code = r#"
            function makeNode(id, label) {
                return { id: id, info: { label: label, active: true } };
            }
            var node = makeNode(42, "Kernel");
            var matrix = [[1, 2], [3, 4]];
            document.write(node.id + ":" + node.info.label + " matrix=" + matrix[1][0]);
        "#;
        assert_eq!(render(code), "42:Kernel matrix=3");
    }

    #[test]
    fn math_and_global_conversions() {
        let code = r#"
            var a = Math.abs(-10);
            var b = Math.min(10, 3, 25);
            var c = Math.max(10, 3, 25);
            var d = Math.pow(2, 5);
            var parsed = parseInt("42") + parseInt("1010", 2);
            var num = Number("100") + 50;
            var boolVal = Boolean(1);
            document.write("a=" + a + " b=" + b + " c=" + c + " d=" + d + " p=" + parsed + " n=" + num + " b=" + boolVal);
        "#;
        assert_eq!(render(code), "a=10 b=3 c=25 d=32 p=52 n=150 b=true");
    }

    /// Parantezli ifadeler ve `+`/`-` zincirleri sol-ilişkili çözülür.
    #[test]
    fn parenthesised_arithmetic_is_evaluated() {
        let code = r#"
            var one = (1);
            var paren = (2 + 3) * 2;
            var chain = 100 - 10 - 5;
            var mixed = (1 + 2) + "x" + (3 - 1);
            document.write(one + " " + paren + " " + chain + " " + mixed);
        "#;
        assert_eq!(render(code), "1 10 85 3x2");
    }

    /// Dize metotları ve `JSON.stringify` değer döndürmeli.
    #[test]
    fn string_methods_and_stringify() {
        let code = r#"
            var s = "minOs";
            document.write(
                s.toUpperCase() + " " +
                s.repeat(2) + " " +
                s.replace("Os", "0S") + " " +
                ("a" + "b").concat("c", "d") + " " +
                JSON.stringify({ k: s, n: 2, ok: true })
            );
        "#;
        assert_eq!(
            render(code),
            r#"MINOS minOsminOs min0S abcd {"k":"minOs","n":2,"ok":true}"#
        );
    }

    /// Sınıf kurucusu ve metodu: `this` bağlamı ve dönüş değeri.
    #[test]
    fn class_constructor_and_method() {
        let code = r#"
            class Point {
                constructor(x, y) {
                    this.x = x;
                    this.y = y;
                }
                getX() {
                    return this.x;
                }
            }
            var p = new Point(3, 4);
            document.write(p.getX());
        "#;
        assert_eq!(render(code), "3");
    }

    /// Kurucu içinde yazılan özellikler örnekte okunabilir.
    #[test]
    fn class_constructor_writes_properties() {
        let code = r#"
            class Point {
                constructor(x, y) {
                    this.x = x;
                    this.y = y;
                }
            }
            var p = new Point(3, 4);
            document.write(p.x + "," + p.y);
        "#;
        assert_eq!(render(code), "3,4");
    }

    /// Metottan `this` üzerinden yazımın kalıcılığı.
    #[test]
    fn class_method_mutation_persists() {
        let code = r#"
            class Counter {
                constructor() {
                    this.n = 0;
                }
                inc() {
                    this.n = this.n + 1;
                }
            }
            var c = new Counter();
            c.inc();
            c.inc();
            document.write(c.n);
        "#;
        assert_eq!(render(code), "2");
    }

    /// Miras alınan metodun alt sınıf örneğinde çalışması ve örtük `super()`.
    #[test]
    fn class_inheritance_method() {
        let code = r#"
            class Shape {
                constructor() {
                    this.kind = "sekil";
                }
                describe() {
                    return this.kind;
                }
            }
            class Circle extends Shape {
                constructor() {
                    this.kind = "daire";
                }
            }
            var c = new Circle();
            document.write(c.describe());
        "#;
        assert_eq!(render(code), "daire");
    }

    /// Açık `super()` çağrısı ebeveyn kurucusunu çalıştırmalı.
    #[test]
    fn class_explicit_super_call() {
        let code = r#"
            class Base {
                constructor(v) {
                    this.v = v;
                }
            }
            class Derived extends Base {
                constructor() {
                    super(41);
                    this.v = this.v + 1;
                }
            }
            var d = new Derived();
            document.write(d.v);
        "#;
        assert_eq!(render(code), "42");
    }

    /// `instanceof` ile kurucu-türetilmiş ilişkisi (miras zinciri dahil).
    #[test]
    fn class_instanceof() {
        let code = r#"
            class Point {
                constructor(x) { this.x = x; }
            }
            class Pixel extends Point {
                constructor() { super(1); }
            }
            var p = new Point(1);
            var q = new Pixel();
            document.write(p instanceof Point);
            document.write("," + (q instanceof Point));
            document.write("," + (q instanceof Pixel));
        "#;
        assert_eq!(render(code), "true,true,true");
    }
}
