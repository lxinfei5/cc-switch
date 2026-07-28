# DESIGN BLUEPRINT: "Glacier" — refined layered glass, disciplined

Chosen by judge panel: Glacier base (Elegance 9/10, Dark-mode 9/10) + Vellum restraint/token-hygiene (9/10) + Sequoia fit (9/10).
Source: design workflow wf_9f698dc0-9df.

## Concept
Evolve the existing .glass vocabulary into a true layered material system: faintly-tinted app backdrop derived from the primary token (so backdrop-blur has something to blur), cards as frosted slabs (1px hairline + top inner highlight + theme-aware shadow), ONE cobalt accent used sparingly. Header stays 64px, add button keeps circular signature shape, glass recipes are token-derived, decorative tics (hover:-translate-y-px, glow shadows, gradient chips) deleted.

## :root (src/index.css)
--background: 220 20% 97%;  --foreground: 222 22% 12%;
--card: 0 0% 100%;          --card-foreground: 222 22% 12%;
--popover: 0 0% 100%;       --popover-foreground: 222 22% 12%;
--primary: 213 92% 50%;     --primary-foreground: 0 0% 100%;
--secondary: 220 14% 95%;   --secondary-foreground: 222 22% 12%;
--muted: 220 14% 95%;       --muted-foreground: 220 9% 42%;
--accent: 220 14% 94%;      --accent-foreground: 222 22% 12%;
--destructive: 0 72% 51%;   --destructive-foreground: 0 0% 100%;
--border: 220 13% 88%;      --input: 220 13% 88%;  --ring: 213 92% 50%;
--radius: 0.625rem;
--success: 158 64% 38%;     --success-foreground: 0 0% 100%;
--warning: 35 92% 44%;      --warning-foreground: 0 0% 100%;
--shadow-1: 0 1px 2px hsl(222 30% 10% / 0.05), 0 1px 1px hsl(222 30% 10% / 0.04);
--shadow-2: 0 4px 16px -4px hsl(222 30% 10% / 0.08), 0 2px 6px -2px hsl(222 30% 10% / 0.05);
--shadow-3: 0 16px 48px -12px hsl(222 30% 10% / 0.16), 0 4px 12px -4px hsl(222 30% 10% / 0.08);
--ease-out-expo: cubic-bezier(0.22, 1, 0.36, 1);

## .dark
--background: 222 16% 9%;   --foreground: 220 15% 94%;
--card: 222 13% 12.5%;      --card-foreground: 220 15% 94%;
--popover: 222 13% 14%;     --popover-foreground: 220 15% 94%;
--primary: 213 100% 64%;    --primary-foreground: 222 30% 10%;
--secondary: 222 11% 16%;   --secondary-foreground: 220 15% 94%;
--muted: 222 11% 16%;       --muted-foreground: 220 10% 62%;
--accent: 222 12% 18%;      --accent-foreground: 220 15% 94%;
--destructive: 0 63% 55%;   --destructive-foreground: 0 0% 100%;
--border: 220 10% 22%;      --input: 220 10% 24%;  --ring: 213 100% 64%;
--success: 160 60% 52%;     --success-foreground: 160 80% 10%;
--warning: 38 95% 58%;      --warning-foreground: 38 90% 10%;
--shadow-1: 0 1px 2px hsl(0 0% 0% / 0.4);
--shadow-2: 0 8px 24px -8px hsl(0 0% 0% / 0.55);
--shadow-3: 0 24px 64px -16px hsl(0 0% 0% / 0.65);

## Backdrop wash (token-derived)
body::before { content:""; position:fixed; inset:0; z-index:-1; pointer-events:none;
  background: radial-gradient(120% 80% at 50% -20%, hsl(var(--primary)/0.05), transparent 60%), hsl(var(--background)); }
.dark body::before { background: radial-gradient(120% 80% at 50% -20%, hsl(var(--primary)/0.07), transparent 60%), hsl(var(--background)); }
App root container: bg-background -> bg-transparent.

## Glass API (rewrite in place; ~30 call sites upgrade free)
.glass { background: hsl(var(--card)/0.6); backdrop-filter: blur(16px) saturate(150%); border:1px solid hsl(var(--border)/0.6); }
.glass-card { background: hsl(var(--card)/0.72); backdrop-filter: blur(20px) saturate(160%); border:1px solid hsl(var(--border)/0.8); box-shadow: var(--shadow-1), inset 0 1px 0 hsl(0 0% 100%/0.9); }
.dark .glass-card { background: linear-gradient(160deg, hsl(0 0% 100%/0.055), hsl(0 0% 100%/0.02)); border:1px solid hsl(0 0% 100%/0.07); box-shadow: var(--shadow-1), inset 0 1px 0 hsl(0 0% 100%/0.06); }
.glass-card-active { border-color: hsl(var(--primary)/0.4); background: hsl(var(--primary)/0.06); box-shadow: 0 4px 16px -6px hsl(var(--primary)/0.25), inset 0 1px 0 hsl(0 0% 100%/0.9); }
.glass-card-success { border-color: hsl(var(--success)/0.4); background: hsl(var(--success)/0.06); box-shadow: 0 4px 16px -6px hsl(var(--success)/0.25), inset 0 1px 0 hsl(0 0% 100%/0.9); }
.dark .glass-card-active,.dark .glass-card-success { box-shadow: var(--shadow-1); border-color: hsl(var(--primary)/0.45); }
.dark .glass-card-success { border-color: hsl(var(--success)/0.45); }
.glass-header { background: hsl(var(--background)/0.72); backdrop-filter: blur(20px) saturate(180%); border-bottom:1px solid hsl(var(--border)/0.5); }

## New utilities
.toolbar-btn { inline-flex h-8 items-center justify-center gap-1.5 rounded-lg px-2 text-muted-foreground transition-colors duration-150 hover:bg-foreground/[0.06] hover:text-foreground; }
.tnum { font-variant-numeric: tabular-nums; }
::selection { background: hsl(var(--primary)/0.25); }
Slim scrollbars (replace display:none): * { scrollbar-width:thin; scrollbar-color: hsl(var(--muted-foreground)/0.25) transparent; } ::-webkit-scrollbar{width/height:8px} thumb hsl(var(--muted-foreground)/0.25) radius 999px, hover /0.4.
DELETE: global *:focus-visible outline rule; old scrollbar display:none; old flat .glass-header. z-scale comment: drag-bar 70 > header 50 > overlays 40.

## tailwind.config.cjs
- colors: add success/warning {DEFAULT,foreground}; DELETE partial blue/gray/green/red/amber overrides (ship atomically with P0 primitive sweep).
- boxShadow: {1:var(--shadow-1),2:var(--shadow-2),3:var(--shadow-3),sm:var(--shadow-1),md:var(--shadow-2),lg:var(--shadow-3)}.
- borderRadius derived: sm calc(radius-0.25rem), md calc(-0.125rem), lg var(--radius), xl calc(+0.375rem), 2xl calc(+0.625rem).
- fontFamily.sans single source: [-apple-system,BlinkMacSystemFont,Inter,Segoe UI,PingFang SC,Hiragino Sans GB,Microsoft YaHei,Noto Sans CJK SC,sans-serif]. Keep mono.
- transitionTimingFunction: {expo: var(--ease-out-expo)}. Durations 150/200.
- plugins: [require("tailwindcss-animate")] + install (fixes dead animate-in/zoom-in Radix overlay classes). Retune overlays to duration-200 zoom-in-97 fade-in-0 ease-expo.
- src/fonts/custom-fonts.css -> comments-only; remove import from main.tsx.

## Typography
Root stays 16px; body{font-size:13px;line-height:1.5}. Brand wordmark text-lg semibold tracking-tight. View titles text-lg semibold tracking-[-0.015em]. Card titles text-sm semibold (ui/card.tsx CardTitle text-2xl -> text-base font-semibold tracking-tight). Metadata text-xs text-muted-foreground. Microcopy floor text-[11px] font-medium (text-[10px] banned). Weights 400/500/600 only. Numerics .tnum (not font-mono except API keys/endpoints/code/model IDs). Replace star/money emoji with lucide Star/Coins.

## Spacing / radius / elevation
4px grid; gutters px-6; card padding p-4; card gaps gap-3; sections space-y-6. <main> sole scroll owner (remove nested scroller overflow, root pb-4, scroller pb-12 px-1; single pb-8). Header 64px, three clusters separated by h-4 w-px bg-border/60. Segmented pills bg-muted rounded-xl p-1 gap-0.5, items h-7 px-2.5 text-xs rounded-lg. Radius roles: buttons/inputs/chips rounded-lg; cards/panels/popovers rounded-xl; segmented rounded-xl + rounded-lg items; rounded-full ONLY add button/avatars/status dots. Elevation 3 layers: L0 wash, L1 .glass-card, L2 overlays bg-popover/95 backdrop-blur-[28px] border-border/60 shadow-3 rounded-xl. Card hover shadow-1->shadow-2 + border-border. Blur <=28px; header only always-blurred. Touch targets >=28px.

## Motion
Two durations 150 micro / 200 structural; easing ease-expo everywhere. Radix overlays 200ms zoom-in-97 fade-in-0 ease-expo. View transitions: collapse 3 nested opacity fades into ONE (opacity+y 6->0, 200ms expo); inner app fades 150ms opacity. Segmented active tab: ONE framer-motion layoutId pill (spring stiffness 500 damping 40), kept out of useAutoCompact path. Hover = color/shadow 150ms only; press active:scale-[0.98] 100ms. Kill transition-all (33 sites) -> transition-colors. Focus ONE mechanism: focus-visible:ring-2 ring-ring/40 ring-offset-1 ring-offset-background; delete global outline + badge ring-2/offset-2. prefers-reduced-motion guard.

## Component plan
P0 (one commit, ~70% win): index.css + tailwind.config + install tailwindcss-animate + custom-fonts no-op; SAME commit sweep ui/button.tsx (bg-blue/red/emerald-500 -> bg-primary/destructive; delete "mcp" variant), ui/tabs.tsx (active bg-blue-500 -> AppSwitcher idiom), ui/switch.tsx (checked -> bg-primary), ui/input.tsx (focus ring-ring/40), ui/card.tsx (CardTitle), ui/badge.tsx (status variants info/success/warning/neutral rounded-md).
P1 shell (App.tsx, AppSwitcher.tsx): .glass-header; 3 clusters + dividers; .toolbar-btn for ~15 ghost strings; add button rounded-full h-8 w-8 bg-primary (kill orange+shadow-orange); brand pill glass-card + shadow-1 (kill rgba glows), icon chip solid bg-primary (takeover bg-success), kill hover:-translate-y-px; scroll fix + root bg-transparent; view transition compose + layoutId glide.
P2 provider cards (ProviderCard/ProviderList): ProviderCard glass-card rounded-xl; active/takeover -> glass-card-active/success (kill bespoke emerald/blue borders + gradient overlays); 7 chips -> badge variants; emoji->lucide; ProviderList amber box -> border-warning/30 bg-warning/10 text-warning; shadow-md->shadow-2.
P3 TPS+footers: TpsMonitorPanel flat boxes->glass-card; icons text-blue/emerald->primary/success; stats font-mono->.tnum; ConcurrencyStat rounded-xl; extract ui/Panel (rounded-xl glass-card px-4 py-3) for UsageFooter x2 + 3 quota footers; inline width hacks->grid-cols-12; numerics .tnum.
P4 settings/overlays/long tail: ThemeSettings segmented->AppSwitcher idiom; delete border-white/10 overrides on .glass; dialogs/popovers L2 recipe + free animations; incremental sweep of ~69 files hardcoded palette utils (text-blue-500->primary, emerald/green->success, amber/yellow->warning, gray->muted-foreground); grep bg-white/rgba(255,255,255 after off-white bg; MarkdownEditor hexes->tokens.

## Anti-goals
No hardcoded colors (palette = primary+success+warning+destructive+neutrals). No decorative motion. No new material variants beyond the 5 glass recipes. No flat-misnomer classes. No radius drift. No type noise (no text-[10px]/uppercase microcopy/weight>600/font-mono stats/emoji). No structural churn (header 64px, no left rail, no global footer, root 16px). No blur excess. No hidden scrollbars. No split focus/selection.

## Verification gates
Eyeball dark card 12.5% on bg 9% w/ white/7 border on hardware (nudge to 13.5% if gray slab). Test scroll-under-header blur, CJK locale (zh/ja) after font reorder, Linux WebKitGTK blur caps, ProviderCard chips at narrow width w/ useAutoCompact + 11px floor.
