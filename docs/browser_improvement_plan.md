# MinOS Web Browser Improvement Plan

## Overview
This plan outlines comprehensive improvements for the MinOS web browser's HTML5, CSS3, and JavaScript support. The browser is a core component of the MinOS operating system, providing web rendering capabilities for applications, documentation, and system information.

## Current State
The browser already supports:
- HTML5 tokenization and layout engine
- CSS3 styling (colors, backgrounds, borders, flexbox, grid, shadows, transitions, animations)
- JavaScript engine (minJs) with basic operators, loops, arrays, objects
- Image formats (PNG, JPEG, GIF, WebP, BMP, SVG)
- TLS 1.3 HTTPS, HTTP/2 partial
- Bookmarks, history, downloads
- minos:// protocol shortcuts

## Improvement Priorities (Order of Implementation)

### Phase 1: HTML5 Forms & Input Types [P1]
**Goal**: Complete form element support for web applications

**Current Limitations**:
- Basic input types only (text, password, checkbox, radio)
- No value tracking or form submission
- Limited <select>/<option> support
- No <label> for attribute handling

**Required Enhancements**:
1. Complete <input> type support:
   - text (already partial)
   - password (already partial)
   - checkbox (already partial)
   - radio (already partial)
   - file - file upload button
   - hidden - hidden input
   - button - clickable button
   - submit - form submit button
   - reset - form reset button
   - number - numeric input
   - email - email input validation
   - tel - telephone input
   - date - date picker
   - search - search field
   - range - slider input

2. <form> element:
   - Basic form submission support
   - GET/POST method handling
   - action attribute processing

3. <select> and <option>:
   - Proper dropdown rendering
   - option value attribute support
   - Selected state tracking

4. <textarea>:
   - Multi-line text support (already partial)
   - Placeholder text handling

5. <label> element:
   - for attribute binding
   - Click label to focus associated input

**Files to Modify**:
- `src/apps/browser/html.rs` - Enhanced tokenizer and layout
- `src/apps/browser/types.rs` - Add form element types and states

---

### Phase 2: CSS3 Enhancements [P1]
**Goal**: Modern CSS features for responsive, rich styling

**Current Limitations**:
- Basic box-shadow and text-shadow
- Missing CSS filters
- Missing aspect-ratio, clamp/min/max
- Limited functional notation support

**Required Enhancements**:
1. CSS Filter Functions:
   - `blur()`, `brightness()`, `contrast()`, `saturate()`
   - `sepia()`, `hue-rotate()`, `invert()`
   - CSS `filter` property

2. CSS Sizing Functions:
   - `clamp(min, val, max)` - constrained value
   - `min(val1, val2, ...)` - minimum of values
   - `max(val1, val2, ...)` - maximum of values

3. CSS Aspect Ratio:
   - `aspect-ratio` property
   - Automatic height calculation from width

4. CSS Masking:
   - `mask-image` property
   - Mask source (image/gradient/clip)

5. CSS Unit Improvements:
   - `ch` unit (width of "0" character)
   - `rem` with proper root element reference
   - `vh`/`vw` with correct viewport reference

6. Additional CSS Properties:
   - `text-decoration-line`, `text-decoration-color`, `text-decoration-style`
   - `caret-color` - cursor color in inputs
   - `user-select` - text selection control
   - `resize` - element resize control
   - `outline` shorthand improvements

**Files to Modify**:
- `src/apps/browser/css.rs` - Enhanced parser and style applier
- `src/apps/browser/types.rs` - Add CSS filter, aspect-ratio, clamp/min/max types

---

### Phase 3: JavaScript Engine [P1]
**Goal**: Comprehensive minJs engine improvements

**Current Limitations**:
- Basic operators and control flow
- Limited array methods (.push, .pop, .shift, .unshift)
- Basic math and string helpers
- No Promise support
- No setTimeout/setInterval
- No fetch/XMLHttpRequest

**Required Enhancements**:
1. Promise Support:
   - `Promise` constructor
   - `.then()`, `.catch()`, `.finally()` methods
   - `async`/`await` keywords
   - `Promise.resolve()`, `Promise.reject()`
   - `Promise.all()`, `Promise.race()`, `Promise.allSettled()`

2. APIs:
   - `setTimeout()` and `clearTimeout()`
   - `setInterval()` and `clearInterval()`
   - `requestAnimationFrame()` and `cancelAnimationFrame()`
   - Basic `fetch()` API (GET/POST)
   - `XMLHttpRequest` with readyState 1-4

3. Additional Array Methods:
   - `.every()`, `.some()`, `.find()`, `.findIndex()`
   - `.filter()`, `.map()` (already partial)
   - `.reduce()`, `.reduceRight()`
   - `.includes()`, `.indexOf()`, `.lastIndexOf()`

4. Additional Object Methods:
   - `Object.entries()`, `Object.keys()`, `Object.values()`
   - `Object.fromEntries()`
   - `Object.hasOwn()`, `Object.prototype.hasOwnProperty()`

5. Additional String Methods:
   - `.repeat()`, `.startsWith()`, `.endsWith()`
   - `.padStart()`, `.padEnd()`
   - `.trimStart()`, `.trimEnd()`

6. Additional Math Methods:
   - `Math.trunc()`, `Math.sign()`, `Math.clamp()`
   - `Math.log()`, `Math.log10()`, `Math.log2()`
   - `Math.cbrt()`, `Math.hypot()`, `Math.pow()` (already partial)

7. Language Features:
   - Default parameter functions
   - Rest parameters (`...args`)
   - Spread operator (`...`)
   - Template literals (basic)
   - Try/Catch with error objects

**Files to Modify**:
- `src/apps/js.rs` - Enhanced JsRuntime implementation
- Add Promise, Timeout, Fetch classes

---

### Phase 4: Browser UI Features [P2]
**Goal**: Enhanced browsing experience

**Required Enhancements**:
1. Tabbed Browsing:
   - Multiple tab interface
   - Tab switching (Ctrl+Tab, Ctrl+Shift+Tab)
   - New tab page
   - Close tab (Ctrl+W)

2. Find-in-Page:
   - Ctrl+F search
   - Highlight matching text
   - Next/previous navigation
   - Case-sensitive option

3. Reader Mode:
   - Strip article of navigation, ads, sidebars
   - Simplified stylesheet
   - Default font scaling

4. Print Support:
   - Print preview
   - PDF-like export
   - Page setup

5. Bookmarks Management:
   - Edit bookmarks
   - Folder organization
   - Import/export

**Files to Modify**:
- `src/apps/browser/mod.rs` - Add tab management
- `src/apps/browser/devtools.rs` - Enhance dev tools
- Add new UI module for tab bar

---

### Phase 5: Responsive Design [P2]
**Goal**: Mobile-friendly web rendering

**Required Enhancements**:
1. Viewport Meta Tag:
   - `<meta name="viewport" content="width=device-width, initial-scale=1.0">` support
   - Scaling controls
   - Maximum/minimum scale restrictions

2. CSS Media Queries:
   - `@media` rule support
   - `min-width`, `max-width`
   - `min-height`, `max-height`
   - `color`, `color-index`, `monochrome`, `resolution` media features

3. Fluid Layout Support:
   - Percentage-based widths
   - `max-width`, `min-width` constraints
   - `width: auto` improvements

4. Mobile Optimization:
   - Touch target sizing (minimum 48px)
   - Horizontal scrolling prevention
   - Portrait/landscape detection

**Files to Modify**:
- `src/apps/browser/html.rs` - Viewport meta tag parsing
- `src/apps/browser/css.rs` - Media query parser
- Add viewport state to BrowserApp struct

---

## Success Metrics
- [ ] 90%+ HTML5 form elements render correctly
- [ ] CSS3 filters, aspect-ratio, clamp/min/max work in-browser
- [ ] JS promises, timeout, fetch APIs functional
- [ ] Viewport meta tag properly handled for mobile layouts
- [ ] Media queries work for responsive design
- [ ] Zero crashes form complex web pages
- [ ] Performance: page load < 5s for simple pages