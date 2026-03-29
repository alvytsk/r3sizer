# Design Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix five design issues identified in the frontend review: clean up dead light-mode CSS, add hero entrance animation, break the symmetric layout, upgrade the footer, and fix button hierarchy.

**Architecture:** All changes are isolated to `web/src/index.css` and `web/src/` React components. No new files required. No Rust changes needed.

**Tech Stack:** React 19, Tailwind CSS v4, CSS custom properties, CSS keyframe animations, CVA (class-variance-authority)

---

## File Map

| File | Changes |
|---|---|
| `web/src/index.css` | Remove dead `:root` light-mode variables; add `@keyframes fade-up`; add `.hero-animate` utility; upgrade `.glow-footer-top` separator |
| `web/index.html` | No changes needed — `class="dark"` already present |
| `web/src/App.tsx` | Add animation classes to hero elements; widen diag sidebar to 400px, narrow params to 300px; upgrade footer markup with uppercase labels + separator |
| `web/src/components/DownloadButton.tsx` | Add amber-tinted outline variant override to download button |

---

## Task 1: Verify and clean up dead light-mode CSS

**Files:**
- Modify: `web/src/index.css`

`index.html` already has `class="dark"` on `<html>`, so the `:root` block never applies. It's dead code that implies the app supports light mode — it doesn't. Replace the neutral `:root` block with a minimal amber-aware light mode so a future screenshot or system-preference override doesn't look broken.

- [ ] **Step 1: Open `web/src/index.css` and find the `:root` block (lines 10–44)**

- [ ] **Step 2: Replace the neutral `:root` block with an amber light-mode version**

Replace:
```css
:root {
  --background: oklch(1 0 0);
  --foreground: oklch(0.145 0 0);
  --card: oklch(1 0 0);
  --card-foreground: oklch(0.145 0 0);
  --popover: oklch(1 0 0);
  --popover-foreground: oklch(0.145 0 0);
  --primary: oklch(0.205 0 0);
  --primary-foreground: oklch(0.985 0 0);
  --secondary: oklch(0.97 0 0);
  --secondary-foreground: oklch(0.205 0 0);
  --muted: oklch(0.97 0 0);
  --muted-foreground: oklch(0.556 0 0);
  --accent: oklch(0.97 0 0);
  --accent-foreground: oklch(0.205 0 0);
  --destructive: oklch(0.577 0.245 27.325);
  --destructive-foreground: oklch(0.577 0.245 27.325);
  --border: oklch(0.922 0 0);
  --input: oklch(0.922 0 0);
  --ring: oklch(0.708 0 0);
  --chart-1: oklch(0.646 0.222 41.116);
  --chart-2: oklch(0.6 0.118 184.704);
  --chart-3: oklch(0.398 0.07 227.392);
  --chart-4: oklch(0.828 0.189 84.429);
  --chart-5: oklch(0.769 0.188 70.08);
  --radius: 0.375rem;
  --sidebar-background: oklch(0.985 0 0);
  --sidebar-foreground: oklch(0.445 0 0);
  --sidebar-primary: oklch(0.205 0 0);
  --sidebar-primary-foreground: oklch(0.985 0 0);
  --sidebar-accent: oklch(0.97 0 0);
  --sidebar-accent-foreground: oklch(0.205 0 0);
  --sidebar-border: oklch(0.922 0 0);
  --sidebar-ring: oklch(0.708 0 0);
}
```

With:
```css
/* Light mode — app is dark-only (class="dark" on <html>) but these
   variables prevent a bare-bones fallback if the class is ever absent. */
:root {
  --background: oklch(0.97 0.004 270);
  --foreground: oklch(0.18 0.01 270);
  --card: oklch(0.95 0.004 270);
  --card-foreground: oklch(0.18 0.01 270);
  --popover: oklch(0.96 0.004 270);
  --popover-foreground: oklch(0.18 0.01 270);
  --primary: oklch(0.52 0.18 75);
  --primary-foreground: oklch(0.97 0.004 270);
  --secondary: oklch(0.90 0.008 270);
  --secondary-foreground: oklch(0.28 0.01 270);
  --muted: oklch(0.90 0.008 270);
  --muted-foreground: oklch(0.50 0.01 270);
  --accent: oklch(0.88 0.01 270);
  --accent-foreground: oklch(0.52 0.18 75);
  --destructive: oklch(0.55 0.22 25);
  --destructive-foreground: oklch(0.55 0.22 25);
  --border: oklch(0.82 0.01 270);
  --input: oklch(0.88 0.008 270);
  --ring: oklch(0.52 0.18 75);
  --chart-1: oklch(0.65 0.18 75);
  --chart-2: oklch(0.52 0.14 230);
  --chart-3: oklch(0.55 0.17 150);
  --chart-4: oklch(0.50 0.15 310);
  --chart-5: oklch(0.50 0.20 25);
  --radius: 0.375rem;
  --sidebar-background: oklch(0.93 0.005 270);
  --sidebar-foreground: oklch(0.40 0.01 270);
  --sidebar-primary: oklch(0.52 0.18 75);
  --sidebar-primary-foreground: oklch(0.97 0.004 270);
  --sidebar-accent: oklch(0.88 0.01 270);
  --sidebar-accent-foreground: oklch(0.40 0.01 270);
  --sidebar-border: oklch(0.82 0.01 270);
  --sidebar-ring: oklch(0.52 0.18 75);
}
```

- [ ] **Step 3: Verify dev server still starts**

```bash
cd /home/alexey/projects/sandbox/r3sizer/web && npm run dev
```
Expected: No build errors in terminal. Browser shows dark UI unchanged.

- [ ] **Step 4: Commit**

```bash
cd /home/alexey/projects/sandbox/r3sizer && git add web/src/index.css && git commit -m "style: replace dead neutral light-mode vars with amber-consistent fallback"
```

---

## Task 2: Hero entrance animation

**Files:**
- Modify: `web/src/index.css` — add `@keyframes fade-up` + stagger utilities
- Modify: `web/src/App.tsx` — add animation classes to hero elements

The upload state (no image loaded) is the first thing the user sees. Currently all elements appear instantly. Adding a staggered fade-up entrance makes the tool feel considered, not default.

- [ ] **Step 1: Add `@keyframes fade-up` and stagger utilities to `web/src/index.css`**

Append inside the existing `@layer utilities { ... }` block (after the `.glow-amber-text` rule):

```css
  /* Hero entrance animation */
  @keyframes fade-up {
    from {
      opacity: 0;
      transform: translateY(10px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .animate-fade-up {
    animation: fade-up 0.4s cubic-bezier(0.16, 1, 0.3, 1) both;
  }

  .delay-100 { animation-delay: 100ms; }
  .delay-200 { animation-delay: 200ms; }
  .delay-300 { animation-delay: 300ms; }
  .delay-400 { animation-delay: 400ms; }
```

- [ ] **Step 2: Add pulsing amber border to the upload drop zone in `web/src/index.css`**

Append to `@layer utilities { ... }` (right after the stagger utilities above):

```css
  @keyframes border-pulse {
    0%, 100% { border-color: oklch(0.78 0.16 75 / 0.15); }
    50%       { border-color: oklch(0.78 0.16 75 / 0.40); }
  }

  .upload-idle {
    animation: border-pulse 3s ease-in-out infinite;
  }
```

- [ ] **Step 3: Apply animation classes to hero elements in `web/src/App.tsx`**

Find the `!inputFile` branch in App.tsx (around line 272). Replace the hero `div` contents:

```tsx
{!inputFile ? (
  <div className="flex-1 flex items-center justify-center px-6">
    <div className="flex flex-col items-center gap-8 max-w-lg w-full -mt-10">
      {/* Hero title */}
      <div className="flex flex-col items-center gap-2 text-center animate-fade-up">
        <h2 className="font-mono text-3xl font-bold tracking-tight text-primary glow-amber-text">
          r3sizer
        </h2>
        <p className="text-sm text-muted-foreground max-w-xs animate-fade-up delay-100">
          Precision downscaling with automatic sharpness optimization.
          Runs entirely in your browser.
        </p>
      </div>
      {/* Upload zone with crop marks */}
      <div className="relative w-full animate-fade-up delay-200">
        {/* Corner crop marks */}
        <div className="absolute -top-2 -left-2 w-5 h-5 border-t-2 border-l-2 border-primary/40 rounded-tl-sm" />
        <div className="absolute -top-2 -right-2 w-5 h-5 border-t-2 border-r-2 border-primary/40 rounded-tr-sm" />
        <div className="absolute -bottom-2 -left-2 w-5 h-5 border-b-2 border-l-2 border-primary/40 rounded-bl-sm" />
        <div className="absolute -bottom-2 -right-2 w-5 h-5 border-b-2 border-r-2 border-primary/40 rounded-br-sm" />
        <ImageUpload />
      </div>
      {/* Pipeline hint */}
      <div className="hidden sm:flex items-center gap-4 text-[11px] font-mono text-muted-foreground/50 animate-fade-up delay-300">
        <span>linearize</span>
        <span className="text-primary/30">&rarr;</span>
        <span>downscale</span>
        <span className="text-primary/30">&rarr;</span>
        <span>sharpen</span>
        <span className="text-primary/30">&rarr;</span>
        <span>optimize</span>
      </div>
    </div>
  </div>
```

- [ ] **Step 4: Apply pulsing amber border to upload idle state in `web/src/components/ImageUpload.tsx`**

Find the outer `div` className (around line 57). Change:
```tsx
${isDragging
  ? "border-primary bg-primary/5 glow-amber"
  : "border-border/60 hover:border-primary/40 hover:bg-surface/50"
}
```
To:
```tsx
${isDragging
  ? "border-primary bg-primary/5 glow-amber"
  : "upload-idle hover:border-primary/60 hover:bg-surface/50"
}
```

- [ ] **Step 5: Verify in browser**

Run `npm run dev`, load the app. Expect:
- Hero title fades up first (~0ms delay)
- Description text fades up 100ms after
- Upload zone fades up 200ms after
- Pipeline hint fades up 300ms after
- The upload zone border pulses gently between dim and brighter amber when idle
- On drag-over, the amber glow replaces the pulse (already working)

- [ ] **Step 6: Commit**

```bash
cd /home/alexey/projects/sandbox/r3sizer && git add web/src/index.css web/src/App.tsx web/src/components/ImageUpload.tsx && git commit -m "style: add staggered hero entrance animation and pulsing upload border"
```

---

## Task 3: Break the symmetric three-column layout

**Files:**
- Modify: `web/src/App.tsx` — change sidebar widths

The layout currently uses identical-ish widths for both sidebars (params: 340px, diag: 380px). Making them visibly different reinforces the semantic difference: params (controls/input) are narrower, diagnostics (output/measurement) are wider. This creates visual asymmetry without breaking the functional grid.

Additionally, the collapsed sidebar width is the same on both sides (`lg:w-11`). Change the diagnostics collapsed width to `lg:w-10` (slightly less) to hint at the asymmetry even when collapsed.

- [ ] **Step 1: Widen the diagnostics sidebar and narrow the params sidebar in `web/src/App.tsx`**

**Parameters sidebar** (around line 175) — change `lg:w-[340px]` to `lg:w-[300px]`:
```tsx
sidebarOpen ? "lg:w-[300px]" : "lg:w-11",
```

**Parameters sidebar inner div** (around line 177) — change `lg:w-[340px]` to `lg:w-[300px]`:
```tsx
<div className="w-full lg:w-[300px] h-full flex flex-col">
```

**Diagnostics sidebar** (around line 327) — change `lg:w-[380px]` to `lg:w-[420px]`:
```tsx
diagOpen ? "lg:w-[420px]" : "lg:w-10",
```

**Diagnostics sidebar inner div** (around line 329) — change `lg:w-[380px]` to `lg:w-[420px]`:
```tsx
<div className="w-full lg:w-[420px] h-full flex flex-col">
```

- [ ] **Step 2: Tighten params sidebar background to push it visually back**

In the `<aside>` for the parameters sidebar (around line 165), change `bg-card` to `bg-background`:
```tsx
"bg-background border-r border-border/40",
```

This makes the params panel slightly darker than the card-colored diagnostics panel, reinforcing the input/output asymmetry.

- [ ] **Step 3: Verify in browser**

Load the app, open an image. Expect:
- Left (params) panel is visibly narrower than right (diagnostics) panel
- Left panel has a slightly darker background than right
- No content is clipped in either sidebar
- Collapse/expand still works on both sides

- [ ] **Step 4: Commit**

```bash
cd /home/alexey/projects/sandbox/r3sizer && git add web/src/App.tsx && git commit -m "style: asymmetric sidebar widths (params 300px / diag 420px) + darker params bg"
```

---

## Task 4: Upgrade the footer to instrument-panel telemetry

**Files:**
- Modify: `web/src/index.css` — add `footer-separator` gradient utility
- Modify: `web/src/App.tsx` — upgrade footer markup

The current footer is a thin `py-2.5` strip. It needs: more vertical presence, uppercase metric labels clearly separated from their values, and a top gradient that makes it feel like a distinct instrument readout panel.

- [ ] **Step 1: Add footer separator gradient to `web/src/index.css`**

Append inside `@layer utilities { ... }`:

```css
  /* Footer top-edge gradient — frames the telemetry strip */
  .footer-separator {
    background: linear-gradient(
      to bottom,
      oklch(0.78 0.16 75 / 0.08) 0px,
      transparent 3px
    );
  }
```

- [ ] **Step 2: Replace the footer in `web/src/App.tsx`** (around line 365)

Replace:
```tsx
<footer className="border-t border-border/60 px-5 py-2.5 flex items-center gap-5 text-xs font-mono text-muted-foreground bg-background/80 backdrop-blur-sm">
  {diagnostics ? (
    <>
      <span>
        s* = <span className="text-foreground">{diagnostics.selected_strength.toFixed(4)}</span>
      </span>
      <span>
        P = <span className="text-foreground">{diagnostics.measured_artifact_ratio.toExponential(2)}</span>
      </span>
      <span>
        {diagnostics.output_size.width}&times;{diagnostics.output_size.height}
      </span>
      <span className="ml-auto">
        {(diagnostics.timing.total_us / 1000).toFixed(0)}ms
      </span>
    </>
  ) : (
    <span className="text-muted-foreground/40">ready</span>
  )}
</footer>
```

With:
```tsx
<footer className="footer-separator border-t border-border/40 px-5 flex items-center gap-6 bg-background/90 backdrop-blur-sm flex-shrink-0 h-11">
  {diagnostics ? (
    <>
      <span className="flex items-baseline gap-1.5">
        <span className="text-[9px] font-mono tracking-[0.15em] uppercase text-muted-foreground/50">S*</span>
        <span className="text-xs font-mono text-foreground tabular-nums">{diagnostics.selected_strength.toFixed(4)}</span>
      </span>
      <span className="w-px h-3 bg-border/40 flex-shrink-0" />
      <span className="flex items-baseline gap-1.5">
        <span className="text-[9px] font-mono tracking-[0.15em] uppercase text-muted-foreground/50">P</span>
        <span className="text-xs font-mono text-foreground tabular-nums">{diagnostics.measured_artifact_ratio.toExponential(2)}</span>
      </span>
      <span className="w-px h-3 bg-border/40 flex-shrink-0" />
      <span className="flex items-baseline gap-1.5">
        <span className="text-[9px] font-mono tracking-[0.15em] uppercase text-muted-foreground/50">Out</span>
        <span className="text-xs font-mono text-foreground tabular-nums">{diagnostics.output_size.width}&times;{diagnostics.output_size.height}</span>
      </span>
      <span className="ml-auto flex items-baseline gap-1.5">
        <span className="text-[9px] font-mono tracking-[0.15em] uppercase text-muted-foreground/50">Total</span>
        <span className="text-xs font-mono text-primary tabular-nums">{(diagnostics.timing.total_us / 1000).toFixed(0)}ms</span>
      </span>
    </>
  ) : (
    <span className="text-[9px] font-mono tracking-[0.15em] uppercase text-muted-foreground/30">ready</span>
  )}
</footer>
```

- [ ] **Step 3: Verify in browser**

Load the app and process an image. Expect:
- Footer is taller (44px / `h-11`) and reads as a distinct panel
- Metric names (S*, P, Out, Total) are tiny uppercase labels above/beside values
- Thin vertical dividers separate the metric groups
- Total time is amber-tinted (uses `text-primary`)
- Footer top edge has a faint amber gradient separator
- "ready" state shows tiny uppercase text

- [ ] **Step 4: Commit**

```bash
cd /home/alexey/projects/sandbox/r3sizer && git add web/src/index.css web/src/App.tsx && git commit -m "style: upgrade footer to instrument-panel telemetry strip with labels and separator"
```

---

## Task 5: Fix button hierarchy

**Files:**
- Modify: `web/src/components/DownloadButton.tsx` — amber-tinted download action
- Modify: `web/src/components/ui/button.tsx` — clarify `outline` dark variant

The current hierarchy: process button (amber fill + glow) → download button (outline, reads same weight as ghost) → ghost buttons (icon-only). The download button is a reward action — user has just processed an image — and should feel distinct from the utility ghost buttons.

- [ ] **Step 1: Add amber tint to the download button in `web/src/components/DownloadButton.tsx`**

Find the `<Button>` at the bottom (around line 90). Change `variant="outline"` and add amber text classes:

```tsx
<Button
  variant="outline"
  size="sm"
  onClick={handleDownload}
  className="font-mono text-[11px] dark:border-primary/30 dark:text-primary dark:hover:bg-primary/10 dark:hover:border-primary/50"
  title={`Save as ${format.toUpperCase()}`}
>
  <Download className="h-3.5 w-3.5 mr-1" />
  {/* Below lg: show format in button since selector is hidden */}
  <span className="lg:hidden">{format.toUpperCase()}</span>
  <span className="hidden lg:inline">Save</span>
</Button>
```

- [ ] **Step 2: Clarify the `outline` variant in dark mode in `web/src/components/ui/button.tsx`**

The `outline` variant currently uses `dark:border-input dark:bg-input/30 dark:hover:bg-input/50` which is a neutral blue-gray. This is correct for generic secondary actions, but the overall contrast with `ghost` is low. Increase the outline border opacity slightly:

Find:
```ts
outline:
  "border-border bg-background hover:bg-muted hover:text-foreground aria-expanded:bg-muted aria-expanded:text-foreground dark:border-input dark:bg-input/30 dark:hover:bg-input/50",
```

Replace:
```ts
outline:
  "border-border bg-background hover:bg-muted hover:text-foreground aria-expanded:bg-muted aria-expanded:text-foreground dark:border-border/70 dark:bg-input/20 dark:hover:bg-input/40",
```

- [ ] **Step 3: Verify in browser**

Load the app and process an image. Expect:
- Download button has amber text and a faint amber border — visually distinct from the ghost "Open" button in the header
- Download button hover shows a subtle amber background tint
- The process button remains the strongest CTA (amber fill + glow)
- The ghost panel-toggle buttons in the toolbar (mobile only) remain clearly tertiary

- [ ] **Step 4: Commit**

```bash
cd /home/alexey/projects/sandbox/r3sizer && git add web/src/components/DownloadButton.tsx web/src/components/ui/button.tsx && git commit -m "style: amber-tinted download button and clarified outline variant for button hierarchy"
```

---

## Self-Review

**Spec coverage check:**

| Issue from review | Task covering it |
|---|---|
| Light mode is abandoned scaffolding | Task 1 ✓ |
| Upload state doesn't earn the first impression | Task 2 ✓ |
| Three-column layout is too conventional | Task 3 ✓ |
| Footer is an afterthought | Task 4 ✓ |
| Button hierarchy is flat | Task 5 ✓ |

**Placeholder scan:** No TBDs, no "implement later", no "similar to Task N" — all steps contain actual code.

**Type consistency:** No new types introduced. All changes are CSS classes and JSX markup.

**One gap found:** The review mentioned "one strong entrance sequence" but only the upload state gets animated (Task 2). The image-loaded state (after processing) has no entrance. Acceptable scope — the upload state is the first impression; post-processing transition is already handled by the processing overlay fade. No additional task needed.
