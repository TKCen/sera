# Browser Agent Extended Capabilities Spec

> Research document for `sera-browser` container capabilities beyond basic actions.
> Covers CDP domains, Playwright/Puppeteer APIs, and patterns from existing AI browser agent frameworks.

## Table of Contents

1. [Network Interception & Monitoring](#1-network-interception--monitoring)
2. [DOM & Accessibility](#2-dom--accessibility)
3. [Performance & Resource Monitoring](#3-performance--resource-monitoring)
4. [Storage & State](#4-storage--state)
5. [Console & Error Capture](#5-console--error-capture)
6. [File Handling](#6-file-handling)
7. [Authentication Flows](#7-authentication-flows)
8. [Media & Canvas](#8-media--canvas)
9. [Geolocation & Device Emulation](#9-geolocation--device-emulation)
10. [Advanced Interaction](#10-advanced-interaction)
11. [iframe & Popup Handling](#11-iframe--popup-handling)
12. [Tab & Window Management](#12-tab--window-management)
13. [AI Browser Agent Framework Patterns](#13-ai-browser-agent-framework-patterns)
14. [Recommended Tool Schema Summary](#14-recommended-tool-schema-summary)

---

## 1. Network Interception & Monitoring

### 1.1 Request/Response Logging

- **What:** Capture all HTTP requests and responses (URL, method, status, headers, body, timing).
- **Why:** Agents need to understand what API calls a page makes, debug failed requests, extract data from XHR/fetch responses (e.g., JSON API payloads that populate the UI), and verify that form submissions succeeded.
- **API:** CDP `Network.requestWillBeSent`, `Network.responseReceived`, `Network.loadingFinished` + `Network.getResponseBody`. Playwright: `page.on('request')`, `page.on('response')`, `response.body()`.
- **Priority:** **P0** — Essential for agents that need to extract structured data from SPAs where content is loaded via API calls rather than rendered in the DOM.

### 1.2 Request Interception & Modification

- **What:** Intercept outgoing requests and modify headers, URL, method, or body before they reach the server. Can also abort requests entirely.
- **Why:** Agents may need to add auth tokens, modify API parameters, block tracking/analytics requests to reduce noise, or inject custom headers for testing.
- **API:** CDP `Fetch.enable` + `Fetch.requestPaused` + `Fetch.continueRequest`/`Fetch.fulfillRequest`/`Fetch.failRequest`. Playwright: `page.route(pattern, handler)` with `route.continue({ headers })`, `route.abort()`, `route.fulfill()`.
- **Priority:** **P1** — Very useful for auth injection and ad/tracker blocking, but not needed for basic browsing.

### 1.3 Response Mocking

- **What:** Intercept responses and replace them with custom data (status, headers, body).
- **Why:** Agents testing workflows can mock API responses to simulate error conditions, test edge cases, or bypass rate limits during development. Also useful for injecting data into pages.
- **API:** CDP `Fetch.fulfillRequest`. Playwright: `route.fulfill({ status, contentType, body })`.
- **Priority:** **P2** — Primarily useful for testing/development agent scenarios.

### 1.4 Request Blocking by Pattern

- **What:** Block requests matching URL patterns (e.g., all `*.analytics.com/*`, `*/ads/*`).
- **Why:** Reduces page load time, removes distracting ad content from screenshots/DOM, lowers bandwidth in sandbox environments. Critical for agent efficiency.
- **API:** CDP `Network.setBlockedURLs`. Playwright: `page.route('**/*.{png,jpg}', route => route.abort())`.
- **Priority:** **P1** — Significantly improves agent performance by reducing page noise.

### 1.5 HAR Recording

- **What:** Record all network activity as a HAR (HTTP Archive) file.
- **Why:** Agents can export a full session replay for debugging, auditing, or reproducing issues. Valuable for agent observability.
- **API:** Playwright: `browser.newContext({ recordHar: { path } })`, `context.close()` to flush. Puppeteer: manual via CDP events or `puppeteer-har` library.
- **Priority:** **P2** — Nice for debugging but not needed in the hot path.

### 1.6 WebSocket Inspection

- **What:** Monitor WebSocket connection lifecycle and capture frames (sent and received).
- **Why:** Many modern apps use WebSockets for real-time data (chat, notifications, live updates). Agents interacting with these apps need to read WS messages.
- **API:** CDP `Network.webSocketCreated`, `Network.webSocketFrameSent`, `Network.webSocketFrameReceived`. Playwright: `page.on('websocket')`, `ws.on('framereceived')`, `ws.on('framesent')`.
- **Priority:** **P1** — Important for agents working with real-time web applications.

---

## 2. DOM & Accessibility

### 2.1 Accessibility Tree Snapshot

- **What:** Get the full accessibility tree (AX tree) of the page — a structured representation of all elements with their roles, names, values, states, and relationships.
- **Why:** This is the single most important capability for AI agents. The AX tree provides a semantic, compact representation of the page that is far more useful to an LLM than raw HTML. It maps directly to what a screen reader sees, giving agents role-based understanding of UI elements.
- **API:** CDP `Accessibility.getFullAXTree`, `Accessibility.queryAXTree`. Playwright: `page.accessibility.snapshot({ interestingOnly: true })`.
- **Priority:** **P0** — This is the primary perception mechanism for AI browser agents. Every major framework uses it.

### 2.2 ARIA-Based Element Querying

- **What:** Find elements by their ARIA role, name, or other accessibility attributes rather than CSS selectors.
- **Why:** ARIA queries are more robust than CSS selectors (which break when class names change) and more semantically meaningful for agents. `getByRole('button', { name: 'Submit' })` is stable across UI redesigns.
- **API:** Playwright: `page.getByRole()`, `page.getByLabel()`, `page.getByPlaceholder()`, `page.getByText()`, `page.getByTitle()`, `page.getByAltText()`, `page.getByTestId()`. CDP: `Accessibility.queryAXTree({ name, role })`.
- **Priority:** **P0** — Dramatically improves agent reliability over CSS/XPath selectors.

### 2.3 Element Bounding Box & Visibility

- **What:** Get the pixel coordinates, dimensions, and visibility state of any element.
- **Why:** Agents need to know where elements are on screen for click targeting, whether elements are visible (not hidden by CSS, not scrolled out of view), and whether elements overlap.
- **API:** CDP `DOM.getBoxModel`, `DOM.getContentQuads`. Playwright: `element.boundingBox()`, `element.isVisible()`, `element.isEnabled()`, `element.isEditable()`.
- **Priority:** **P0** — Required for reliable click targeting and understanding page layout.

### 2.4 Computed Styles Inspection

- **What:** Read the computed CSS styles of any element (color, font, visibility, display, etc.).
- **Why:** Agents may need to determine if an element is visually hidden (opacity: 0, display: none), check color contrast for accessibility auditing, or understand layout relationships.
- **API:** CDP `CSS.getComputedStyleForNode`. Playwright: `element.evaluate(el => getComputedStyle(el))`.
- **Priority:** **P2** — Useful for accessibility-focused agents but not core navigation.

### 2.5 DOM Mutation Observation

- **What:** Watch for changes in the DOM — elements added, removed, attributes changed, text content modified.
- **Why:** Agents need to detect when a page has finished updating after an action (e.g., clicking a button triggers an AJAX load). Also useful for detecting toast notifications, modal dialogs appearing, and dynamic content loads.
- **API:** CDP `DOM.setChildNodeCount`, `DOM.childNodeInserted`, `DOM.attributeModified`. Playwright: `page.waitForSelector()`, `page.waitForFunction()`, `element.waitFor({ state: 'attached' })`.
- **Priority:** **P1** — Important for robust action-then-wait patterns.

### 2.6 Shadow DOM Traversal

- **What:** Pierce shadow DOM boundaries to access elements inside web components.
- **Why:** Many modern UIs use web components with shadow DOM (e.g., Salesforce Lightning, GitHub, YouTube). Without shadow DOM access, agents cannot interact with these elements.
- **API:** CDP: `DOM.describeNode({ pierce: true })`. Playwright: uses `>>` combinator or `page.locator('my-component').locator('internal:shadow=button')`. Puppeteer: `element.shadowRoot`.
- **Priority:** **P1** — Required for any modern web app using web components.

---

## 3. Performance & Resource Monitoring

### 3.1 Page Load Metrics

- **What:** Capture Core Web Vitals and navigation timing — FCP, LCP, CLS, TTFB, domContentLoaded, load event timing.
- **Why:** Agents performing web testing or monitoring need to measure page performance. Also useful for detecting when a page is "ready" for interaction.
- **API:** CDP `Performance.getMetrics`, `Performance.enable`. Playwright: `page.evaluate(() => performance.getEntriesByType('navigation'))`.
- **Priority:** **P2** — Relevant for testing/monitoring agents, not general browsing.

### 3.2 Network Waterfall / Resource Timing

- **What:** Get detailed timing breakdown for each resource (DNS, connect, TLS, TTFB, download).
- **Why:** Performance debugging — helps agents identify slow resources, blocked requests, or CDN issues.
- **API:** CDP `Network.responseReceived` includes timing. Playwright: `response.serverAddr()`, `response.securityDetails()`, or `page.evaluate(() => performance.getEntriesByType('resource'))`.
- **Priority:** **P2** — Niche use case.

### 3.3 JavaScript Heap / Memory Monitoring

- **What:** Get JS heap statistics and detect memory leaks.
- **Why:** Long-running agent sessions may cause memory pressure. Monitoring helps decide when to restart the browser context.
- **API:** CDP `Runtime.getHeapUsage`, `HeapProfiler.takeHeapSnapshot`. Puppeteer: `page.metrics()` returns JSHeapUsedSize/JSHeapTotalSize.
- **Priority:** **P2** — Operational concern, not agent logic.

### 3.4 CPU Profiling

- **What:** Start/stop CPU profiling to identify heavy JS execution.
- **Why:** Debug slow pages or detect infinite loops that would hang an agent's browsing session.
- **API:** CDP `Profiler.start`, `Profiler.stop`, `Profiler.setSamplingInterval`.
- **Priority:** **P2** — Edge case debugging.

---

## 4. Storage & State

### 4.1 Cookie Management

- **What:** Read, write, and delete cookies. Filter by domain, path, name. Handle secure/httpOnly/sameSite attributes.
- **Why:** Agents need to manage authentication state, persist sessions across page navigations, handle consent cookies, and clear cookies to test logged-out flows.
- **API:** CDP `Network.getCookies`, `Network.setCookie`, `Network.deleteCookies`. Playwright: `context.cookies()`, `context.addCookies()`, `context.clearCookies()`.
- **Priority:** **P0** — Essential for session management and auth persistence.

### 4.2 localStorage / sessionStorage

- **What:** Read, write, and clear localStorage and sessionStorage for any origin.
- **Why:** Many SPAs store auth tokens, user preferences, and application state in web storage. Agents need to read these (e.g., extract JWT tokens) and write them (e.g., set feature flags).
- **API:** CDP `DOMStorage.getDOMStorageItems`, `DOMStorage.setDOMStorageItem`, `DOMStorage.removeDOMStorageItem`. Playwright: `page.evaluate(() => localStorage.getItem('key'))`.
- **Priority:** **P1** — Important for SPA auth token management.

### 4.3 IndexedDB Inspection

- **What:** Read the contents of IndexedDB databases (list databases, object stores, query records).
- **Why:** PWAs and complex SPAs store significant data in IndexedDB (cached API responses, offline data, application state). Agents may need to read or clear this data.
- **API:** CDP `IndexedDB.requestDatabaseNames`, `IndexedDB.requestData`, `IndexedDB.clearObjectStore`. Playwright: via `page.evaluate()` using the IndexedDB API.
- **Priority:** **P2** — Needed only for complex PWA interactions.

### 4.4 Service Worker Management

- **What:** List, inspect, start, stop, and unregister service workers. Bypass service worker caching.
- **Why:** Service workers can intercept network requests and serve cached content, which may confuse agents expecting fresh data. Agents need to detect and optionally bypass them.
- **API:** CDP `ServiceWorker.enable`, `ServiceWorker.unregister`, `ServiceWorker.inspectWorker`. Playwright: `context.serviceWorkers()`, or bypass with `{ serviceWorkers: 'block' }` on context creation.
- **Priority:** **P1** — Important for ensuring agents see fresh content.

### 4.5 Browser Context / State Isolation

- **What:** Create isolated browser contexts (incognito-like) with separate cookie jars, storage, and cache.
- **Why:** Agents performing multi-account workflows need complete state isolation. Also critical for running multiple agent browser sessions in parallel without cookie/cache leakage.
- **API:** Playwright: `browser.newContext()`. Puppeteer: `browser.createBrowserContext()`. CDP: `Target.createBrowserContext`.
- **Priority:** **P0** — Essential for multi-agent and multi-account scenarios in SERA.

### 4.6 Cache Control

- **What:** Clear HTTP cache, disable caching entirely.
- **Why:** Agents may need to force fresh content loads, especially when testing deployments or interacting with pages that have aggressive caching.
- **API:** CDP `Network.clearBrowserCache`, `Network.setCacheDisabled(true)`. Playwright: implicit per-context (fresh cache by default).
- **Priority:** **P1** — Important for deterministic agent behavior.

---

## 5. Console & Error Capture

### 5.1 Console Message Capture

- **What:** Capture all `console.log`, `console.warn`, `console.error`, `console.info`, `console.debug` output from the page.
- **Why:** Console output reveals application state, debug info, API errors, and validation messages. Critical for agents that need to understand why something failed on the page.
- **API:** CDP `Runtime.consoleAPICalled`. Playwright: `page.on('console', msg => ...)`. Puppeteer: `page.on('console')`.
- **Priority:** **P0** — Essential for debugging agent interactions.

### 5.2 JavaScript Error & Exception Capture

- **What:** Capture uncaught exceptions, unhandled promise rejections, and JS errors.
- **Why:** Agents need to detect when their actions caused errors (e.g., clicking a button threw a JS exception). Distinguishes between "action succeeded" and "action appeared to succeed but broke the page."
- **API:** CDP `Runtime.exceptionThrown`. Playwright: `page.on('pageerror', error => ...)`. Puppeteer: `page.on('pageerror')`.
- **Priority:** **P0** — Critical for reliable agent error detection.

### 5.3 Page Crash Detection

- **What:** Detect when a page/tab crashes (out of memory, renderer crash).
- **Why:** Agents need to detect and recover from page crashes, which would otherwise leave them in an undefined state.
- **API:** CDP `Inspector.targetCrashed`. Playwright: `page.on('crash')`. Puppeteer: `page.on('error')`.
- **Priority:** **P1** — Important for agent resilience.

---

## 6. File Handling

### 6.1 File Download Interception

- **What:** Detect file downloads, control save location, read downloaded file content, cancel downloads.
- **Why:** Agents may need to download files (reports, exports, documents) and then process their content. Need to know when downloads start, complete, and where the file ends up.
- **API:** CDP `Browser.setDownloadBehavior`, `Page.downloadWillBegin`, `Page.downloadProgress`. Playwright: `page.on('download')`, `download.saveAs()`, `download.path()`, `download.failure()`.
- **Priority:** **P0** — Many agent workflows involve downloading files.

### 6.2 File Upload / File Chooser

- **What:** Programmatically handle native file upload dialogs by injecting file paths without user interaction.
- **Why:** Agents need to upload files as part of workflows (e.g., uploading a document to a web form). The native file dialog is not automatable via DOM — requires special handling.
- **API:** CDP `DOM.setFileInputFiles`. Playwright: `page.on('filechooser')`, `fileChooser.setFiles()`, `element.setInputFiles()`. Puppeteer: `elementHandle.uploadFile()`.
- **Priority:** **P0** — File upload is a common workflow action.

### 6.3 Drag-and-Drop File Upload

- **What:** Simulate dropping files onto drop zones (drag-and-drop upload areas).
- **Why:** Many modern UIs use drag-and-drop upload zones instead of (or alongside) file input elements. Agents need to support both patterns.
- **API:** Playwright: `page.dispatchEvent(target, 'drop', { dataTransfer })` (requires constructing a DataTransfer). CDP: `Input.dispatchDragEvent`.
- **Priority:** **P1** — Increasingly common UI pattern.

---

## 7. Authentication Flows

### 7.1 HTTP Basic/Digest Auth

- **What:** Provide credentials for HTTP authentication dialogs.
- **Why:** Many internal/enterprise tools use HTTP basic auth. Agents need to authenticate without manual intervention.
- **API:** CDP `Fetch.requestPaused` + `Fetch.continueWithAuth`. Playwright: `context.setHTTPCredentials({ username, password })` or `page.on('dialog')`.
- **Priority:** **P1** — Common for enterprise/internal tools.

### 7.2 Client Certificate Handling

- **What:** Provide client TLS certificates for mutual TLS (mTLS) authentication.
- **Why:** Enterprise environments and government sites often require client certificates.
- **API:** Playwright: `browser.newContext({ clientCertificates: [{ origin, certPath, keyPath }] })` (v1.46+). Puppeteer: launch flag `--ignore-certificate-errors` or use CDP `Security.setIgnoreCertificateErrors`.
- **Priority:** **P2** — Enterprise edge case.

### 7.3 Certificate Error Handling

- **What:** Accept or reject invalid/self-signed TLS certificates.
- **Why:** Agents accessing internal tools, dev environments, or self-signed HTTPS services need to bypass certificate warnings.
- **API:** CDP `Security.setIgnoreCertificateErrors`. Playwright: `browser.newContext({ ignoreHTTPSErrors: true })`.
- **Priority:** **P1** — Common for internal tool access.

### 7.4 OAuth / SSO Flow Support

- **What:** Handle multi-step OAuth flows involving redirects, popups, and consent screens across multiple domains.
- **Why:** Logging into web apps often requires OAuth flows (Google, GitHub, Microsoft). Agents need to navigate these multi-page flows.
- **API:** Playwright: `page.waitForURL(pattern)` to detect redirects, `context.on('page')` to handle OAuth popups. No special CDP domain — it is handled through normal navigation + popup interception.
- **Priority:** **P1** — Most SaaS tools require OAuth login.

### 7.5 Storage State Persistence

- **What:** Export and import complete browser authentication state (cookies + localStorage + sessionStorage) as a JSON file.
- **Why:** Agents can log in once and reuse the auth state across sessions, avoiding repeated login flows. Critical for efficient long-running agents.
- **API:** Playwright: `context.storageState({ path })` (export), `browser.newContext({ storageState: path })` (import). Puppeteer: manual cookie export/import.
- **Priority:** **P0** — Massive efficiency gain for agents. Avoids re-authentication on every session.

---

## 8. Media & Canvas

### 8.1 Screenshot Variants

- **What:** Full page screenshot (including below the fold), element-specific screenshot, viewport-only screenshot, with configurable format (PNG, JPEG, WebP) and quality.
- **Why:** Full-page screenshots let agents see content not in the viewport. Element screenshots focus on specific areas for detailed analysis. Format/quality control manages payload size.
- **API:** CDP `Page.captureScreenshot({ captureBeyondViewport, clip })`. Playwright: `page.screenshot({ fullPage, clip, type, quality })`, `element.screenshot()`.
- **Priority:** **P0** — Screenshots are a primary perception mechanism alongside the AX tree.

### 8.2 PDF Generation

- **What:** Render the current page as a PDF with configurable margins, headers/footers, page size, and scale.
- **Why:** Agents may need to create PDF exports of web content (invoices, reports, articles).
- **API:** CDP `Page.printToPDF`. Playwright: `page.pdf({ path, format, margin, printBackground })`. Only works in headless Chromium.
- **Priority:** **P2** — Useful for specific workflows.

### 8.3 Canvas Content Extraction

- **What:** Extract the pixel data or image representation from `<canvas>` elements.
- **Why:** Many visualization/chart libraries (D3, Chart.js, Three.js) render to canvas. Agents need to capture these visualizations.
- **API:** CDP: `Page.captureScreenshot` with a clip region, or `Runtime.evaluate` calling `canvas.toDataURL()`. Playwright: `elementHandle.screenshot()` works on canvas.
- **Priority:** **P1** — Common for data visualization pages.

### 8.4 Video/Audio Control

- **What:** Play, pause, mute, seek, get duration/currentTime of media elements.
- **Why:** Agents interacting with media platforms (YouTube, Spotify web) need to control playback. Also useful for detecting autoplaying audio/video.
- **API:** CDP: `Runtime.evaluate` calling HTMLMediaElement methods. Playwright: `element.evaluate(el => el.pause())`.
- **Priority:** **P2** — Niche use case.

### 8.5 Screencast / Video Recording

- **What:** Record a video of the browsing session (frames + timing).
- **Why:** Agent observability — replay what the agent did for debugging, auditing, or demonstration.
- **API:** CDP `Page.startScreencast`, `Page.screencastFrame`. Playwright: `context.newPage()` with `recordVideo: { dir }`. Puppeteer: via `page.screencast()` (experimental) or ffmpeg.
- **Priority:** **P1** — Very valuable for agent observability and debugging.

---

## 9. Geolocation & Device Emulation

### 9.1 Viewport / Device Emulation

- **What:** Set viewport dimensions, device scale factor, and mobile device emulation (touch events, device metrics).
- **Why:** Agents may need to interact with mobile-responsive sites, test layouts at different sizes, or emulate specific devices.
- **API:** CDP `Emulation.setDeviceMetricsOverride`. Playwright: `browser.newContext({ viewport: { width, height }, deviceScaleFactor, isMobile, hasTouch })`. Pre-built device descriptors: `playwright.devices['iPhone 14']`.
- **Priority:** **P1** — Important for agents testing responsive designs or interacting with mobile-only features.

### 9.2 Geolocation Spoofing

- **What:** Override the browser's geolocation to report a specific latitude/longitude.
- **Why:** Agents interacting with location-dependent services (maps, delivery apps, local search) need to set their virtual location.
- **API:** CDP `Emulation.setGeolocationOverride`. Playwright: `context.grantPermissions(['geolocation'])` + `context.setGeolocation({ latitude, longitude })`.
- **Priority:** **P1** — Required for location-dependent workflows.

### 9.3 Timezone Override

- **What:** Override the browser's timezone.
- **Why:** Date/time-sensitive applications display different content based on timezone. Agents need to control this for consistent behavior.
- **API:** CDP `Emulation.setTimezoneOverride`. Playwright: `browser.newContext({ timezoneId: 'America/New_York' })`.
- **Priority:** **P1** — Important for consistent date handling.

### 9.4 Locale / Language Override

- **What:** Override the browser's language/locale preferences.
- **Why:** Agents interacting with internationalized sites need to control the language. Also affects number/date formatting.
- **API:** CDP `Emulation.setLocaleOverride`. Playwright: `browser.newContext({ locale: 'fr-FR' })`.
- **Priority:** **P1** — Required for i18n testing and non-English workflows.

### 9.5 Network Condition Throttling

- **What:** Simulate slow network conditions (3G, offline, custom latency/bandwidth).
- **Why:** Testing how apps behave under poor connectivity. Also useful for simulating offline conditions to test PWA behavior.
- **API:** CDP `Network.emulateNetworkConditions`. Playwright: via CDP session `context.newCDPSession(page)`.
- **Priority:** **P2** — Testing-focused.

### 9.6 Color Scheme Emulation

- **What:** Force light/dark mode preference (`prefers-color-scheme`).
- **Why:** Agents may need to interact with dark-mode-specific UIs or capture screenshots in both modes.
- **API:** CDP `Emulation.setEmulatedMedia({ features: [{ name: 'prefers-color-scheme', value: 'dark' }] })`. Playwright: `browser.newContext({ colorScheme: 'dark' })`.
- **Priority:** **P2** — Minor UX concern.

### 9.7 User-Agent Override

- **What:** Set a custom User-Agent string.
- **Why:** Some sites serve different content based on UA. Agents may need to appear as a specific browser/device.
- **API:** CDP `Network.setUserAgentOverride`. Playwright: `browser.newContext({ userAgent: '...' })`.
- **Priority:** **P1** — Important for sites with UA-based content variation.

---

## 10. Advanced Interaction

### 10.1 Drag and Drop

- **What:** Simulate mouse-based drag and drop operations between elements.
- **Why:** Many UIs use drag-and-drop (Trello boards, file managers, form builders, Kanban boards). Cannot be achieved with simple click sequences.
- **API:** CDP `Input.dispatchDragEvent`. Playwright: `page.dragAndDrop(source, target)`, or manually via `mouse.down()` + `mouse.move()` + `mouse.up()`.
- **Priority:** **P1** — Common UI pattern in productivity apps.

### 10.2 Hover States & Tooltips

- **What:** Hover over elements to trigger hover states, tooltips, dropdown menus, and flyouts.
- **Why:** Many UIs hide information or functionality behind hover interactions (tooltip data, dropdown menus, action buttons that appear on hover).
- **API:** CDP `Input.dispatchMouseEvent` (mouseMoved). Playwright: `element.hover()`, `page.hover(selector)`.
- **Priority:** **P0** — Critical for navigating modern UIs with hover-dependent menus.

### 10.3 Keyboard Shortcuts

- **What:** Send arbitrary keyboard combinations (Ctrl+C, Ctrl+S, Alt+Tab, etc.).
- **Why:** Many web apps have keyboard shortcuts (Gmail, Notion, Slack). Agents may need to use them for efficiency or to trigger actions not accessible via click.
- **API:** CDP `Input.dispatchKeyEvent`. Playwright: `page.keyboard.press('Control+A')`, `page.keyboard.type('text')`, `page.keyboard.down('Shift')` / `page.keyboard.up('Shift')`.
- **Priority:** **P1** — Useful for power-user shortcuts in complex apps.

### 10.4 Multi-Step Mouse Operations

- **What:** Fine-grained mouse control — move to coordinates, click-and-hold, draw/paint, precision positioning.
- **Why:** Some UIs require precise mouse operations (drawing tools, sliders, color pickers, map interactions).
- **API:** CDP `Input.dispatchMouseEvent`. Playwright: `page.mouse.move(x, y)`, `page.mouse.down()`, `page.mouse.up()`, `page.mouse.click(x, y, { button: 'right' })`.
- **Priority:** **P1** — Important for interactive web apps.

### 10.5 Touch Events

- **What:** Simulate touch interactions — tap, swipe, pinch, long-press.
- **Why:** Mobile-emulated browsing requires touch events. Some mobile-first web apps respond only to touch, not mouse events.
- **API:** CDP `Input.dispatchTouchEvent`. Playwright: `page.tap()`, requires `hasTouch: true` in context.
- **Priority:** **P2** — Only needed for mobile emulation.

### 10.6 Clipboard Access

- **What:** Read and write the system clipboard.
- **Why:** Agents may need to copy text from the page (especially text that resists selection) or paste content into inputs.
- **API:** Playwright: `page.evaluate(() => navigator.clipboard.readText())` (requires permission). CDP: grant `clipboardReadWrite` permission via `Browser.grantPermissions`.
- **Priority:** **P1** — Useful for data extraction and input.

---

## 11. iframe & Popup Handling

### 11.1 Cross-Frame Interaction

- **What:** Access and interact with elements inside iframes (same-origin and cross-origin).
- **Why:** Many web apps embed content in iframes — payment forms (Stripe), rich text editors, embedded widgets, ads, and captcha challenges. Agents must cross frame boundaries.
- **API:** Playwright: `page.frameLocator('iframe#payment')`, `page.frames()`, `frame.locator()`. Puppeteer: `page.frames()`, `frame.$(selector)`. CDP: each frame gets its own execution context.
- **Priority:** **P0** — Very common; many real-world tasks require iframe interaction.

### 11.2 Popup / New Window Interception

- **What:** Detect when new browser windows or tabs are opened (by `window.open()`, `target="_blank"`, etc.) and get a handle to interact with them.
- **Why:** OAuth flows, payment confirmations, file previews, and many links open in new windows. Agents need to follow these.
- **API:** Playwright: `context.on('page')` or `page.waitForEvent('popup')`. Puppeteer: `browser.on('targetcreated')`, `page.on('popup')`. CDP: `Target.targetCreated`.
- **Priority:** **P0** — Critical for multi-page workflows.

### 11.3 Dialog Handling (alert/confirm/prompt)

- **What:** Automatically handle JavaScript `alert()`, `confirm()`, `prompt()` dialogs and `beforeunload` events.
- **Why:** Dialogs block all page interaction. Agents must be able to dismiss alerts, accept/decline confirmations, and provide text for prompts.
- **API:** Playwright: `page.on('dialog', dialog => dialog.accept())`. Puppeteer: `page.on('dialog')`. CDP: `Page.javascriptDialogOpening` + `Page.handleJavaScriptDialog`.
- **Priority:** **P0** — Dialogs halt agent execution if not handled.

### 11.4 Permission Handling

- **What:** Automatically grant or deny browser permission requests (notifications, geolocation, camera, microphone, clipboard).
- **Why:** Permission prompts block execution. Agents need a policy for auto-granting or auto-denying.
- **API:** Playwright: `context.grantPermissions(['geolocation', 'notifications'])`. CDP: `Browser.grantPermissions`.
- **Priority:** **P1** — Important for smooth agent operation.

---

## 12. Tab & Window Management

### 12.1 Multi-Tab Orchestration

- **What:** Open, close, and switch between multiple browser tabs. Keep track of tab state.
- **Why:** Complex agent workflows may require operating across multiple tabs (e.g., comparing information from two pages, keeping a reference open while filling a form).
- **API:** Playwright: `context.newPage()` creates a new tab, `context.pages()` lists all. Puppeteer: `browser.newPage()`, `browser.pages()`. CDP: `Target.createTarget`, `Target.activateTarget`.
- **Priority:** **P1** — Important for complex multi-step workflows.

### 12.2 Window Sizing & Positioning

- **What:** Resize and reposition the browser window.
- **Why:** Some responsive sites change layout at specific breakpoints. Agents may need specific viewport sizes for reliable element positioning.
- **API:** CDP `Browser.setWindowBounds`. Playwright: `browser.newContext({ viewport: { width, height } })`. Not commonly needed beyond viewport emulation.
- **Priority:** **P2** — Viewport emulation (9.1) covers most needs.

### 12.3 Background Tab Behavior

- **What:** Control whether background tabs are throttled (reduced timer resolution, paused animations).
- **Why:** Chromium throttles background tabs by default, which may cause agents' waitForSelector or timers to behave unexpectedly in non-focused tabs.
- **API:** Launch flag `--disable-background-timer-throttling`, `--disable-backgrounding-occluded-windows`. CDP: no direct domain.
- **Priority:** **P1** — Important for multi-tab agent reliability.

---

## 13. AI Browser Agent Framework Patterns

### 13.1 Browser Use

**Architecture:** Python framework. Agent loop: observe (screenshot + AX tree) -> think (LLM) -> act (Playwright action). Key differentiator: supports multiple LLM backends (OpenAI, Anthropic, local).

**Capabilities exposed:**
- AX tree with element indexing (each interactive element gets an integer ID the LLM can reference)
- Screenshot with element bounding box overlays
- Structured action space: `click(elementId)`, `type(elementId, text)`, `scroll(direction)`, `go_to_url`, `go_back`, `extract_content`, `done(result)`
- Vision mode: LLM sees screenshots with numbered element labels drawn on them
- DOM element extraction with cleaned text content
- Tab management (open, close, switch)
- Cookie persistence for login sessions
- Custom action injection (user-defined tools)

**Patterns:**
- Element indexing (integer IDs mapped to DOM elements) for compact LLM references
- Screenshot annotation with numbered labels
- Action result feedback (success/failure) for agent self-correction
- Configurable max actions per task
- Conversation history management for context window control

### 13.2 LaVague

**Architecture:** Python. Uses a "World Model" (multimodal LLM) for high-level planning and an "Action Engine" for low-level Selenium/Playwright execution.

**Capabilities exposed:**
- HTML chunk extraction with relevance scoring
- Screenshot-based element identification
- Selenium and Playwright backend support
- Code generation approach: LLM generates Python automation code rather than issuing structured actions
- Knowledge base integration (retrieval-augmented generation over previous successful actions)

**Patterns:**
- Two-tier architecture: planner LLM + executor LLM
- Code-generation actions (LLM writes Playwright code, framework executes it)
- Interactive element detection via ML models
- Retrieval of similar past actions for few-shot learning

### 13.3 AgentQL

**Architecture:** Natural language selectors for web elements. Query language that describes elements semantically rather than by CSS/XPath.

**Capabilities exposed:**
- Semantic element querying: `{ search_box }`, `{ login_button }`, `{ product_list[] { name, price } }`
- Structured data extraction from pages using natural language schemas
- Cross-page element tracking (same query works across redesigns)
- Playwright integration as a plugin

**Patterns:**
- Declarative query language for element selection
- Schema-based data extraction
- Element persistence across page changes
- Query compilation to CSS selectors

### 13.4 Stagehand (by Browserbase)

**Architecture:** TypeScript SDK for AI web agents. Three core primitives: `act`, `extract`, `observe`.

**Capabilities exposed:**
- `act(instruction)`: Natural language action execution (e.g., "click the search button")
- `extract(instruction, schema)`: Structured data extraction with Zod schema validation
- `observe(instruction)`: Returns possible actions the agent can take on the current page
- DOM processing and chunking for LLM context limits
- Vision support (screenshot-based understanding)
- Caching of successful action mappings for performance

**Patterns:**
- Three-primitive API (act/extract/observe) as a clean abstraction over browser automation
- Zod schema for typed data extraction
- Observation before action (look before you leap)
- Action caching / memoization
- DOM chunking for context window management

### 13.5 Anthropic Computer Use

**Architecture:** Claude model with native tool use for computer control. Uses screenshot-based perception.

**Capabilities exposed:**
- Screenshot perception (primary modality)
- Mouse actions: click, double-click, drag, move
- Keyboard: type text, press key combinations
- Screenshot with coordinate system for precise targeting
- Cursor position tracking

**Patterns:**
- Coordinate-based interaction (x, y pixel targeting from screenshots)
- Screenshot-as-observation loop
- No DOM/AX tree access (pure vision approach)
- Combines browser with desktop-level control

### 13.6 Cross-Framework Patterns Summary

| Pattern | Used By | Priority for SERA |
|---|---|---|
| AX tree + element IDs | Browser Use, Stagehand | **P0** — primary perception |
| Annotated screenshots | Browser Use, Computer Use | **P0** — secondary perception |
| Structured action space | Browser Use, Stagehand | **P0** — tool interface |
| Observation before action | Stagehand, Browser Use | **P0** — `observe` primitive |
| Structured data extraction | AgentQL, Stagehand | **P0** — `extract` primitive |
| Action result feedback | All | **P0** — error recovery |
| Session/cookie persistence | Browser Use | **P0** — auth efficiency |
| DOM chunking for context | Stagehand | **P1** — context management |
| Action caching/memoization | Stagehand, LaVague | **P1** — performance |
| Code-generation actions | LaVague | **P2** — flexibility vs. safety |
| Dual LLM (planner + executor) | LaVague | **P2** — SERA handles via agents |
| Coordinate-based targeting | Computer Use | **P1** — fallback for complex UIs |

---

## 14. Recommended Tool Schema Summary

Based on the research above, here is a prioritized list of capabilities to add to the `browser-interact` tool beyond the basic actions already planned.

### P0 — Must Have

| Capability | Category | Tool Action Name |
|---|---|---|
| Accessibility tree snapshot | DOM | `get-accessibility-tree` |
| ARIA-based element finding | DOM | `find-element` (with role/name/text) |
| Element bounding box & visibility | DOM | `get-element-info` |
| Console message capture | Console | `get-console-logs` |
| JS error/exception capture | Console | `get-page-errors` |
| Cookie management (CRUD) | Storage | `manage-cookies` |
| Browser context isolation | Storage | `create-context` / `close-context` |
| Storage state export/import | Auth | `save-session` / `load-session` |
| Request/response logging | Network | `get-network-log` |
| File download handling | Files | `handle-download` |
| File upload handling | Files | `upload-file` |
| Full-page screenshot | Media | `screenshot` (extend with fullPage) |
| iframe interaction | Frames | `switch-frame` / `frame-action` |
| Popup/window interception | Frames | `handle-popup` |
| Dialog handling | Frames | `handle-dialog` |
| Hover interaction | Interaction | `hover` |
| Annotated screenshot | Perception | `screenshot-annotated` |
| Observation (available actions) | Perception | `observe` |
| Structured data extraction | Perception | `extract` |

### P1 — Very Useful

| Capability | Category | Tool Action Name |
|---|---|---|
| Request interception/modification | Network | `intercept-request` |
| Request blocking by pattern | Network | `block-requests` |
| WebSocket monitoring | Network | `monitor-websockets` |
| localStorage/sessionStorage | Storage | `manage-storage` |
| Service worker management | Storage | `manage-service-workers` |
| Cache control | Storage | `clear-cache` |
| HTTP Basic auth | Auth | `set-http-credentials` |
| Certificate error bypass | Auth | `ignore-certificate-errors` |
| OAuth flow support | Auth | popup handling + URL wait |
| DOM mutation observation | DOM | `wait-for-change` |
| Shadow DOM traversal | DOM | piercing selectors |
| Viewport/device emulation | Emulation | `set-viewport` |
| Geolocation spoofing | Emulation | `set-geolocation` |
| Timezone override | Emulation | `set-timezone` |
| Locale/language override | Emulation | `set-locale` |
| User-Agent override | Emulation | `set-user-agent` |
| Drag and drop | Interaction | `drag-and-drop` |
| Keyboard shortcuts | Interaction | `press-keys` |
| Multi-step mouse operations | Interaction | `mouse-action` |
| Clipboard access | Interaction | `clipboard` |
| Multi-tab management | Tabs | `open-tab` / `close-tab` / `switch-tab` |
| Background tab throttling | Tabs | launch flag |
| Permission auto-grant | Frames | `set-permissions` |
| Video recording / screencast | Media | `start-recording` / `stop-recording` |
| Canvas content extraction | Media | `capture-canvas` |
| Page crash detection | Console | `on-crash` handler |
| HAR recording | Network | `record-har` |
| Drag-and-drop file upload | Files | extend `upload-file` |

### P2 — Nice to Have

| Capability | Category | Tool Action Name |
|---|---|---|
| Response mocking | Network | `mock-response` |
| IndexedDB inspection | Storage | `inspect-indexeddb` |
| Page load metrics | Performance | `get-performance-metrics` |
| Network waterfall / timing | Performance | `get-resource-timing` |
| JS heap monitoring | Performance | `get-memory-usage` |
| CPU profiling | Performance | `profile-cpu` |
| PDF generation | Media | `generate-pdf` |
| Video/audio control | Media | `media-control` |
| Network throttling | Emulation | `throttle-network` |
| Color scheme emulation | Emulation | `set-color-scheme` |
| Touch events | Interaction | `touch-action` |
| Client certificate handling | Auth | `set-client-certificate` |
| Computed styles inspection | DOM | `get-styles` |
| Window sizing/positioning | Tabs | `resize-window` |

---

## 15. Architecture Recommendations for sera-browser

### 15.1 Perception Layer

Implement a dual perception system:

1. **AX Tree mode (default):** Compact, semantic, token-efficient. Assign integer IDs to interactive elements. Return as structured JSON. This is what the agent uses for most interactions.
2. **Screenshot mode:** Full-page or viewport screenshot with optional bounding box annotations. Used when the AX tree is insufficient (canvas, complex layouts, visual verification).
3. **Hybrid mode:** AX tree + screenshot in a single observation. Most expensive but most complete.

### 15.2 Action Result Feedback

Every action should return a structured result:

```typescript
interface ActionResult {
  success: boolean;
  action: string;
  elementDescription?: string;  // human-readable description of the target
  error?: string;
  screenshot?: string;         // base64 screenshot after action (optional)
  consoleErrors?: string[];    // any JS errors that occurred during action
  networkErrors?: string[];    // any failed network requests during action
  url: string;                 // current URL after action
  title: string;               // current page title after action
}
```

### 15.3 Sandbox Boundary Considerations

The `sera-browser` container should respect SERA tier policies:

| Capability | Tier 1 (Restricted) | Tier 2 (Standard) | Tier 3 (Full) |
|---|---|---|---|
| Navigation | Allowlisted domains only | All HTTP/HTTPS | All protocols |
| File download | Blocked | To workspace only | Unrestricted |
| File upload | From workspace only | From workspace only | Unrestricted |
| Network interception | Read-only logging | Read + block | Full (mock, modify) |
| Cookie/storage | Read only | Read + write (scoped) | Full access |
| Clipboard | Disabled | Read only | Read + write |
| Geolocation | Disabled | Fixed location | Spoofable |
| Code evaluation | Disabled | Read-only eval | Full eval |

### 15.4 Context Window Optimization

Browser observation data can consume enormous token budgets. Recommended strategies:

1. **AX tree filtering:** Return only interactive elements by default. Full tree on request.
2. **Viewport-only content:** Only return elements within the visible viewport unless full page is requested.
3. **Element limit:** Cap returned elements at ~100 with pagination.
4. **Screenshot compression:** JPEG at 60-70% quality for observation screenshots. PNG only when pixel accuracy matters.
5. **Incremental observation:** Return only elements that changed since the last observation.
6. **Network log windowing:** Only return last N requests, with filtering by URL pattern.

### 15.5 Recommended Technology Stack

- **Browser engine:** Chromium via Playwright (preferred over Puppeteer — better API, auto-wait, multi-browser support, better iframe handling, built-in tracing)
- **Container base:** `mcr.microsoft.com/playwright:v1.50.0-noble` (includes Chromium + dependencies)
- **Protocol:** Playwright API as primary interface, with CDP session fallback for capabilities Playwright does not expose directly
- **Communication:** Tool commands via SERA intercom (Centrifugo), or HTTP API within the container
