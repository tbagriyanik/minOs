# Phase 4: Browser UI Features Plan

## Goal: Enhanced browsing experience with tabbed browsing, find-in-page, and reader mode

## Overview
Adding modern browser UI features to the MinOS web browser including tabbed browsing interface, find-in-page search functionality, and reader mode for simplified page viewing.

## Current Browser UI State
- Single tab browsing (no tab interface)
- No find-in-page functionality
- No reader mode
- Basic bookmarks and history

## Required Enhancements

### 1. Tabbed Browsing [P2]
**Features:**
- Multiple tab interface with tab bar
- Tab switching (Ctrl+Tab, Ctrl+Shift+Tab)
- New tab page (about:blank or start page)
- Close tab (Ctrl+W)
- Tab ordering and management

**BrowserApp changes needed:**
- Add `tabs: Vec<BrowserTab>` field
- Add `active_tab: usize` field (already exists, re-purpose)
- Add `close_requested: bool` field
- Add `tab_close_shortcut` handling

**BrowserTab structure:**
- `url: [u8; 128]` - tab URL
- `title: [u8; 64]` - page title
- `favicon: Option<Arc<BackgroundImage>>` - tab icon
- `index: usize` - index in tab list
- `active: bool` - whether tab is active
- `loading: bool` - whether page is loading
- `can_go_back: bool` - history navigation capability
- `can_go_forward: bool` - history navigation capability

**UI changes:**
- Draw tab bar at top of browser window
- Render each tab's title and favicon
- Handle mouse clicks on tabs
- Handle keyboard shortcuts (Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+W)

### 2. Find-in-Page [P2]
**Features:**
- Ctrl+F to open find dialog
- Highlight matching text
- Next/previous navigation (F3, Shift+F3)
- Case-sensitive option
- Highlight all matches

**Implementation:**
- Add find mode to BrowserApp
- Add find query storage
- Add match tracking (current match index, total matches)
- Add highlighting logic during render
- Handle keyboard navigation (F3/Shift+F3)

**BrowserApp changes needed:**
- `find_active: bool` (already exists)
- `find_query: [u8; 64]` (already exists)
- `find_match_index: Cell<usize>` (already exists)
- `find_match_count: Cell<usize>` (already exists)
- New: `find_highlight: Option<(usize, usize)>` - current match position

### 3. Reader Mode [P2]
**Features:**
- Strip article of navigation, ads, sidebars
- Simplified stylesheet (default fonts, sizes)
- Default font scaling
- Print-friendly view
- Toggle with toolbar button

**Implementation:**
- Detect `<article>` tags and extract main content
- Remove `<nav>`, `<aside>`, `<header>`, `<footer>` elements
- Apply simplified CSS (base font size, line height, max width)
- Add reader toggle button to toolbar

**BrowserApp changes needed:**
- `reader_mode: bool` field
- `reader_content: Vec<BrowserLine>` - simplified content
- Reader mode activation/deactivation logic

## UI Implementation Priority

1. **Tabbed browsing** (highest impact - most requested feature)
2. **Find-in-page** (essential for web navigation)
3. **Reader mode** (content-focused browsing)

## Files to Modify

### Primary:
- `src/apps/browser/mod.rs` - Add tab management, find mode, reader mode
- `src/apps/browser/html.rs` - Enhanced rendering for tabs/find/highlight
- `src/apps/browser/types.rs` - Add tab-related types

### UI/Supporting:
- `src/apps/browser/devtools.rs` - May need enhancements
- Consider adding minimal GUI elements for tab bar rendering

## Success Metrics
- [ ] Tab creation and switching works (Ctrl+Tab cycles through tabs)
- [ ] New tab page loads about:blank or start page
- [ ] Ctrl+W closes active tab (or whole browser if single tab)
- [ ] Find-in-page highlights matches and navigates (F3/Shift+F3)
- [ ] Reader mode strips non-content elements
- [ ] No crashes with complex web pages with tabs