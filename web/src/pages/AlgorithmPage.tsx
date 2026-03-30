import { useState, useEffect } from "react";
import { Link } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";
import katex from "katex";
import "katex/dist/katex.min.css";

/* ---------- content data ---------- */

const PIPELINE_STAGES = [
  { name: "linearize", desc: "sRGB \u2192 linear" },
  { name: "downscale", desc: "adaptive kernel" },
  { name: "classify", desc: "region map" },
  { name: "baseline", desc: "measure P(0)" },
  { name: "probe", desc: "N strengths" },
  { name: "fit", desc: "cubic P\u0302(s)" },
  { name: "solve", desc: "Cardano" },
  { name: "sharpen", desc: "adaptive + guard" },
  { name: "evaluate", desc: "cap + score" },
  { name: "encode", desc: "linear \u2192 sRGB" },
] as const;

const DESIGN_DECISIONS = [
  {
    title: "Single-precision pixels, double-precision fitting",
    text: "Image data uses single-precision floats for memory efficiency. Polynomial fitting uses double precision because the Vandermonde normal equations have terms up to s\u2076 \u2014 single precision causes catastrophic cancellation in the 4\u00d74 system.",
  },
  {
    title: "No clamping during sharpening",
    text: "Out-of-range values after sharpening are the artifact signal. Clamping would destroy the information the metric needs. Values are only clamped at the final output stage.",
  },
  {
    title: "Lightness-based sharpening",
    text: "Sharpening CIE Y luminance and reconstructing RGB via k = L\u2032/L preserves chromaticity. This is paper-supported and avoids the color shifts that RGB-channel sharpening introduces.",
  },
  {
    title: "Baseline subtraction",
    text: "Relative-to-base mode subtracts pre-sharpen artifacts from each measurement. This isolates sharpening-induced artifacts from those inherent in the downscale, producing a cleaner fit.",
  },
  {
    title: "Fallback is not an error",
    text: "When the cubic solve finds no root in range, the pipeline falls back to the best probe sample. It always produces a result. The selection outcome is always reported transparently.",
  },
  {
    title: "Immutable base image",
    text: "The downscaled image is never mutated during probing. Each probe produces a fresh allocation, ensuring the final sharpening pass uses the exact same base as the probes.",
  },
  {
    title: "Content-adaptive gain map",
    text: "The classifier labels each pixel as Flat, Textured, StrongEdge, Microtexture, or RiskyHaloZone. A per-class gain table translates this into a per-pixel strength multiplier. Misclassification degrades gracefully \u2014 gain values are bounded to [0.25, 4.0].",
  },
  {
    title: "Chroma guard is non-destructive",
    text: "The chroma guard monitors per-pixel chroma shift after lightness sharpening and applies soft clamping only where the shift exceeds the threshold (default 25%). Per-region factors further tighten the budget for edges and halo zones. It cannot increase saturation, only reduce it back toward the original.",
  },
  {
    title: "Evaluator caps strength, then scores",
    text: "Before final sharpening, the evaluator suggests a maximum strength from image content features (edge density, gradient variance). If the solver\u2019s s* exceeds this cap, it is reduced \u2014 preventing perceptual oversharpening that the gamut metric alone cannot detect (e.g. portraits). After sharpening, the evaluator also predicts a quality score for diagnostics.",
  },
] as const;

const DIAGNOSTICS_FIELDS = [
  ["Input/Output sizes", "Original and target dimensions"],
  ["Probe samples", "All (strength, metric, breakdown) tuples"],
  ["Fit coefficients", "The cubic polynomial a, b, c, d"],
  ["Fit quality", "R\u00b2, residuals, condition number"],
  ["Robustness flags", "Monotonicity, LOO stability"],
  ["Selection mode", "How s* was chosen"],
  ["Fallback reason", "Why polynomial root was bypassed"],
  ["Crossing status", "Where \u0050\u0302(s) intersects P\u2080"],
  ["Metric breakdown", "All four component scores (final output)"],
  ["Metric weights", "Per-component composite score weights"],
  ["Region coverage", "Per-class pixel counts (adaptive mode)"],
  ["Adaptive validation", "Backoff outcome and final scale"],
  ["Per-stage timing", "Microsecond wall-clock per stage"],
  ["Input ingress", "Color-space normalization diagnostics"],
  ["Resize strategy", "Which kernels were used per region"],
  ["Chroma guard", "Fraction clamped, mean/max chroma shift"],
  ["Evaluator result", "Predicted quality score and features"],
  ["Recommendations", "Actionable parameter patches"],
] as const;

const TOC_SECTIONS = [
  { id: "pipeline", label: "Pipeline" },
  { id: "presets", label: "Presets" },
  { id: "stages", label: "Stages" },
  { id: "math", label: "Math" },
  { id: "design", label: "Decisions" },
  { id: "diagnostics", label: "Diagnostics" },
  { id: "assumptions", label: "Assumptions" },
] as const;

/* ---------- scroll spy ---------- */

function useActiveSection(ids: readonly string[]) {
  const [activeId, setActiveId] = useState<string>("");

  useEffect(() => {
    const elements = ids
      .map((id) => document.getElementById(id))
      .filter(Boolean) as HTMLElement[];

    if (elements.length === 0) return;

    const observer = new IntersectionObserver(
      (entries) => {
        // Find the topmost visible section
        const visible = entries
          .filter((e) => e.isIntersecting)
          .sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);

        if (visible.length > 0) {
          setActiveId(visible[0].target.id);
        }
      },
      { rootMargin: "-80px 0px -60% 0px", threshold: 0 },
    );

    elements.forEach((el) => observer.observe(el));
    return () => observer.disconnect();
  }, [ids]);

  return activeId;
}

/* ---------- KaTeX helpers ---------- */

function renderTex(tex: string, displayMode = false) {
  return katex.renderToString(tex, { throwOnError: false, displayMode });
}

function InlineMath({ tex }: { tex: string }) {
  return <span dangerouslySetInnerHTML={{ __html: renderTex(tex) }} />;
}

function MathBlock({ tex }: { tex: string }) {
  return (
    <div className="my-4 rounded-lg bg-surface border border-border/40 overflow-x-auto flex">
      <div className="w-1 flex-shrink-0 rounded-l-lg bg-primary/30" />
      <div className="px-5 py-3.5 min-w-0">
        <div dangerouslySetInnerHTML={{ __html: renderTex(tex, true) }} />
      </div>
    </div>
  );
}

/* ---------- Hero SVG: the P(s) curve ---------- */

function HeroCurve() {
  // Stylized cubic P(s) crossing the threshold P₀
  // Probe dots at measured positions along the curve
  const probePoints = [
    { x: 60, y: 285 },
    { x: 95, y: 278 },
    { x: 145, y: 260 },
    { x: 210, y: 225 },
    { x: 300, y: 165 },
    { x: 385, y: 95 },
    { x: 450, y: 35 },
  ];

  return (
    <svg
      viewBox="0 0 520 320"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className="absolute inset-0 w-full h-full"
      preserveAspectRatio="xMidYMid meet"
      aria-hidden
    >
      {/* Axes */}
      <line x1="40" y1="290" x2="490" y2="290" stroke="currentColor" strokeWidth="1" opacity="0.12" />
      <line x1="40" y1="20" x2="40" y2="290" stroke="currentColor" strokeWidth="1" opacity="0.12" />

      {/* Axis labels */}
      <text x="495" y="294" fontSize="11" fontFamily="monospace" fill="currentColor" opacity="0.15">s</text>
      <text x="28" y="18" fontSize="11" fontFamily="monospace" fill="currentColor" opacity="0.15">P</text>

      {/* Threshold line P₀ */}
      <line
        x1="40" y1="200" x2="490" y2="200"
        stroke="oklch(0.78 0.16 75)" strokeWidth="1" strokeDasharray="6 4" opacity="0.35"
      />
      <text x="494" y="204" fontSize="9" fontFamily="monospace" fill="oklch(0.78 0.16 75)" opacity="0.5">P₀</text>

      {/* The cubic curve */}
      <path
        d="M 40 290 C 100 285, 180 270, 240 230 S 380 110, 470 20"
        stroke="oklch(0.78 0.16 75)" strokeWidth="2" opacity="0.25" fill="none"
      />

      {/* Fill under curve to threshold */}
      <path
        d="M 40 290 C 100 285, 180 270, 240 230 S 380 110, 470 20 L 470 290 Z"
        fill="oklch(0.78 0.16 75)" opacity="0.03"
      />

      {/* s* vertical drop line */}
      <line
        x1="270" y1="200" x2="270" y2="290"
        stroke="oklch(0.78 0.16 75)" strokeWidth="1" strokeDasharray="3 3" opacity="0.3"
      />
      <text x="264" y="303" fontSize="9" fontFamily="monospace" fill="oklch(0.78 0.16 75)" opacity="0.5">s*</text>

      {/* Intersection dot */}
      <circle cx="270" cy="200" r="4" fill="oklch(0.78 0.16 75)" opacity="0.5" />

      {/* Probe sample dots */}
      {probePoints.map((p, i) => (
        <circle
          key={i} cx={p.x} cy={p.y} r="2.5"
          fill="currentColor" opacity="0.15"
        />
      ))}
    </svg>
  );
}

/* ---------- reusable pieces ---------- */

function Tag({ children }: { children: React.ReactNode }) {
  return (
    <span className="inline-block px-2 py-0.5 rounded-md bg-primary/10 text-primary text-[11px] font-mono tracking-wide">
      {children}
    </span>
  );
}


type ChecklistEntry = string | [tex: string, suffix: string];

function Checklist({
  marker,
  markerClass,
  items,
}: {
  marker: string;
  markerClass: string;
  items: ChecklistEntry[];
}) {
  return (
    <ul className="space-y-1 text-sm text-muted-foreground list-none">
      {items.map((item, i) => (
        <li key={i} className="flex gap-2">
          <span className={`${markerClass} mt-0.5`}>{marker}</span>
          <span>
            {Array.isArray(item) ? (
              <><InlineMath tex={item[0]} />{item[1]}</>
            ) : (
              item
            )}
          </span>
        </li>
      ))}
    </ul>
  );
}

function SectionHeading({ id, children }: { id: string; children: React.ReactNode }) {
  return (
    <h2 id={id} className="text-xl font-heading font-bold text-foreground mt-20 mb-4 flex items-center gap-3 scroll-mt-24">
      <span className="w-10 h-px bg-gradient-to-r from-primary/50 to-transparent" />
      {children}
    </h2>
  );
}

function PipelineStep({
  n,
  title,
  children,
}: {
  n: number;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="group relative flex gap-4">
      <div className="flex flex-col items-center">
        <span className="flex-shrink-0 inline-flex items-center justify-center w-7 h-7 rounded-full bg-primary/15 text-primary text-xs font-mono font-bold border border-primary/20">
          {n}
        </span>
        <div className="flex-1 w-px bg-border/30 group-last:hidden mt-2" />
      </div>
      <div className="pb-8 min-w-0">
        <div className="flex items-baseline gap-2 mb-1.5">
          <h3 className="text-base font-heading font-semibold text-foreground">{title}</h3>
        </div>
        <div className="text-sm text-muted-foreground leading-relaxed">{children}</div>
      </div>
    </div>
  );
}

/* ---------- main page ---------- */

const TOC_IDS = TOC_SECTIONS.map((s) => s.id);

export default function AlgorithmPage() {
  const activeSection = useActiveSection(TOC_IDS);

  return (
    <div className="min-h-screen bg-background grain relative">
      {/* Navigation bar */}
      <nav className="sticky top-0 z-20 border-b border-border/60 backdrop-blur-sm bg-background/80">
        <div className="max-w-6xl mx-auto px-6 py-2.5 flex items-center gap-3">
          <Link
            to="/"
            className="flex items-center gap-1.5 text-xs font-mono text-muted-foreground hover:text-primary transition-colors"
          >
            <ArrowLeft className="h-3 w-3" />
            r3sizer
          </Link>
          <div className="h-4 w-px bg-border/40" />
          <span className="text-xs font-mono text-muted-foreground/50">
            algorithm
          </span>
        </div>
      </nav>

      {/* Hero with curve background */}
      <header className="relative overflow-hidden border-b border-border/30">
        <div className="max-w-6xl mx-auto px-6 relative">
          {/* The P(s) curve — the differentiation anchor */}
          <div
            className="absolute right-0 top-0 w-[50%] h-full text-foreground opacity-70 hidden md:block"
            style={{ maskImage: "linear-gradient(to right, transparent 0%, black 40%)", WebkitMaskImage: "linear-gradient(to right, transparent 0%, black 40%)" }}
          >
            <HeroCurve />
          </div>

          <div className="relative z-10 py-16 sm:py-20 max-w-xl">
            <div className="flex items-center gap-3 mb-5 animate-fade-up">
              <Tag>v0.6</Tag>
              <Tag>auto-sharpness</Tag>
            </div>
            <h1 className="text-4xl sm:text-5xl font-heading font-bold text-foreground tracking-tight mb-5 animate-fade-up delay-100">
              The Algorithm
            </h1>
            <p className="text-base sm:text-lg text-muted-foreground leading-relaxed animate-fade-up delay-200">
              Automatically select the optimal sharpening strength when
              downscaling. Fit a cubic polynomial to artifact ratios, then solve
              for maximum sharpness within a perceptual quality budget.
            </p>

            {/* Core constraint — inline callout */}
            <div className="mt-8 rounded-lg border border-primary/20 bg-primary/[0.04] px-5 py-4 animate-fade-up delay-300">
              <p className="text-sm text-primary/90 leading-relaxed">
                <span className="font-bold font-mono">Core constraint</span>{" \u2014 "}
                find <InlineMath tex="s^*" /> maximizing sharpness subject
                to <InlineMath tex="P(s^*) \leq P_0" />, where <InlineMath tex="P_0" /> is
                the target artifact ratio (fraction of color values outside valid gamut).
              </p>
              <p className="text-xs text-primary/60 mt-2">
                Two calibrated presets: <strong>Photo</strong> (<InlineMath tex="P_0 = 0.003" />, default)
                for natural images, and <strong>Precision</strong> (<InlineMath tex="P_0 = 0.001" />)
                for text, UI, and architecture.
              </p>
            </div>
          </div>
        </div>
      </header>

      {/* Body: TOC sidebar + content */}
      <div className="max-w-6xl mx-auto flex">
        {/* Sticky TOC — desktop only */}
        <aside className="hidden xl:block w-44 flex-shrink-0 pt-10 pl-6">
          <nav className="sticky top-16">
            <p className="text-[10px] font-mono tracking-[0.2em] uppercase text-muted-foreground/40 mb-3">
              On this page
            </p>
            <ul className="space-y-0.5 border-l border-border/30">
              {TOC_SECTIONS.map(({ id, label }) => {
                const isActive = activeSection === id;
                return (
                  <li key={id} className="relative">
                    {isActive && (
                      <span className="absolute left-0 top-1 bottom-1 w-px bg-primary transition-all" />
                    )}
                    <a
                      href={`#${id}`}
                      className={[
                        "text-xs font-mono block py-1 pl-3 transition-colors",
                        isActive
                          ? "text-primary"
                          : "text-muted-foreground/50 hover:text-primary",
                      ].join(" ")}
                    >
                      {label}
                    </a>
                  </li>
                );
              })}
            </ul>
          </nav>
        </aside>

        {/* Main content */}
        <main className="flex-1 min-w-0 px-6 pt-4 pb-24 max-w-3xl">

          {/* Pipeline overview */}
          <SectionHeading id="pipeline">Pipeline Overview</SectionHeading>
          <p className="text-sm text-muted-foreground mb-6">
            Every image passes through a fixed sequence of stages in linear RGB.
            sRGB encoding is only applied at output.
          </p>

          {/* Pipeline: stepped vertical grid */}
          <div className="grid grid-cols-2 sm:grid-cols-5 gap-px bg-border/20 rounded-xl overflow-hidden border border-border/30 mb-8">
            {PIPELINE_STAGES.map(({ name, desc }, i) => (
              <div
                key={name}
                className="bg-card px-3 py-3 flex flex-col items-center text-center gap-1 relative"
              >
                <span className="text-[9px] font-mono text-primary/40 absolute top-1.5 left-2.5">
                  {String(i + 1).padStart(2, "0")}
                </span>
                <span className="text-xs font-mono font-medium text-foreground/80 mt-2">
                  {name}
                </span>
                <span className="text-[10px] text-muted-foreground/40">
                  {desc}
                </span>
              </div>
            ))}
          </div>

          {/* Calibrated presets */}
          <SectionHeading id="presets">Calibrated Presets</SectionHeading>
          <p className="text-sm text-muted-foreground leading-relaxed mb-4">
            Two presets are calibrated against an 8-scene corpus spanning text,
            architecture, portraits, foliage, saturated color, low-light noise,
            and mixed street scenes. Both use two-pass adaptive probing, content-adaptive
            sharpening, and chroma guard.
          </p>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 mb-4">
            <div className="rounded-lg border border-primary/30 bg-primary/[0.04] px-4 py-3.5">
              <p className="text-sm font-bold text-foreground mb-1">Photo <span className="text-xs font-normal text-primary/60 ml-1">default</span></p>
              <ul className="space-y-1 text-sm text-muted-foreground list-none">
                <li className="flex gap-2"><span className="text-primary/60">&#9654;</span><span><InlineMath tex="P_0 = 0.003" /> (0.3% artifact budget)</span></li>
                <li className="flex gap-2"><span className="text-primary/60">&#9654;</span><span>Coarse range [0.003, 1.0], 7 probes</span></li>
                <li className="flex gap-2"><span className="text-primary/60">&#9654;</span><span>4 dense probes around the crossing</span></li>
              </ul>
              <p className="text-xs text-muted-foreground/50 mt-2">
                Natural photographs, portraits, landscapes. Allows stronger
                sharpening where the content tolerates it.
              </p>
            </div>
            <div className="rounded-lg border border-border/40 bg-card px-4 py-3.5">
              <p className="text-sm font-bold text-foreground mb-1">Precision</p>
              <ul className="space-y-1 text-sm text-muted-foreground list-none">
                <li className="flex gap-2"><span className="text-primary/60">&#9654;</span><span><InlineMath tex="P_0 = 0.001" /> (0.1% artifact budget)</span></li>
                <li className="flex gap-2"><span className="text-primary/60">&#9654;</span><span>Coarse range [0.003, 0.5], 7 probes</span></li>
                <li className="flex gap-2"><span className="text-primary/60">&#9654;</span><span>4 dense probes around the crossing</span></li>
              </ul>
              <p className="text-xs text-muted-foreground/50 mt-2">
                Screenshots, UI, architecture, text. Tight budget preserves
                hard edges without color fringing.
              </p>
            </div>
          </div>

          <p className="text-sm text-muted-foreground leading-relaxed mb-2">
            Corpus validation confirms two distinct content regimes:
          </p>
          <ul className="space-y-1 text-sm text-muted-foreground list-none mb-2">
            <li className="flex gap-2">
              <span className="text-primary/60">&#9654;</span>
              <span><strong>Artifact-sensitive</strong> (text, architecture) — crossings
                at <InlineMath tex="s^* \approx 0.001\text{--}0.05" /></span>
            </li>
            <li className="flex gap-2">
              <span className="text-primary/60">&#9654;</span>
              <span><strong>Artifact-tolerant</strong> (portraits, low-light) — crossings
                at <InlineMath tex="s^* \approx 0.08\text{--}0.80" /></span>
            </li>
          </ul>
          <p className="text-xs text-muted-foreground/50">
            Mixed-street under Precision exhausting the budget is expected — the
            scene contains text with surrounding photographic content, and the
            strict budget correctly limits sharpening to protect the text.
          </p>

          {/* Stage-by-stage breakdown */}
          <SectionHeading id="stages">Stage Details</SectionHeading>

          <div className="mt-6">
            <PipelineStep n={1} title="Input Decoding & Linearization">
              <p>
                The input image is decoded and normalized to
                {" "}<InlineMath tex="[0,\,1]" />. The IEC 61966-2-1 (sRGB)
                transfer function is applied immediately to convert to linear
                light:
              </p>
              <MathBlock tex={String.raw`f(v) = \begin{cases} \dfrac{v}{12.92} & v \leq 0.04045 \\[6pt] \left(\dfrac{v + 0.055}{1.055}\right)^{2.4} & v > 0.04045 \end{cases}`} />
              <p>
                All subsequent processing operates in this linear space where
                physical light intensity is proportional to pixel value.
              </p>
            </PipelineStep>

            <PipelineStep n={2} title="Downscale">
              <p>
                Lanczos3 resampling reduces the image to the target dimensions
                (default). The kernel is applied in linear light — no gamma
                curve distortion. Values are not clamped at this stage.
              </p>
              <p className="mt-2">
                In content-adaptive resize mode, the source image is classified
                first and a different kernel is applied per region: Gaussian for
                flat areas, Lanczos3 for detail, Mitchell-Netravali for
                halo-prone zones. Results from each kernel are blended by
                per-pixel class assignment.
              </p>
            </PipelineStep>

            <PipelineStep n={3} title="Region Classification">
              <p>
                Active only in content-adaptive sharpening mode. A four-pass
                algorithm labels every pixel by its local content:
              </p>
              <ul className="mt-2 space-y-1 text-sm list-none">
                {[
                  ["Flat", "low gradient, low variance"],
                  ["Textured", "moderate gradient or variance"],
                  ["Strong edge", "high gradient"],
                  ["Microtexture", "high variance, low gradient"],
                  ["Risky halo zone", "high gradient AND high variance"],
                ].map(([cls, desc]) => (
                  <li key={cls} className="flex gap-2">
                    <span className="text-primary/60 mt-0.5">&#9654;</span>
                    <span><strong>{cls}</strong> — {desc}</span>
                  </li>
                ))}
              </ul>
              <p className="mt-2">
                A per-class gain table then converts the region map into a
                per-pixel strength multiplier consumed at the final sharpening
                stage.
              </p>
            </PipelineStep>

            <PipelineStep n={4} title="Contrast Leveling">
              <p className="italic text-muted-foreground/60">
                Optional stage (disabled by default). Applies per-channel 1st–99th
                percentile stretch. Documented placeholder — the exact formula
                from the paper is not yet known.
              </p>
            </PipelineStep>

            <PipelineStep n={5} title="Baseline Measurement">
              <p>
                Before any sharpening, the artifact ratio of the downscaled base
                image is measured. In relative-to-base mode (default), this
                baseline is subtracted from each probe measurement so the
                fitted polynomial only reflects <em>sharpening-induced</em>{" "}
                artifacts, not resize artifacts.
              </p>
            </PipelineStep>

            <PipelineStep n={6} title="Probe Sharpening">
              <p>
                The core exploration phase. The default <strong>two-pass</strong> strategy
                places probes adaptively:
              </p>
              <ul className="mt-2 space-y-1.5 list-none text-sm">
                <li className="flex gap-2">
                  <span className="text-primary/60 mt-0.5">&#9654;</span>
                  <span>
                    <strong>Coarse pass</strong> — 7 probes log-spaced from 0.003 to the
                    preset ceiling (Photo: 1.0, Precision: 0.5). Brackets
                    the <InlineMath tex="P_0" /> crossing.
                  </span>
                </li>
                <li className="flex gap-2">
                  <span className="text-primary/60 mt-0.5">&#9654;</span>
                  <span>
                    <strong>Dense pass</strong> — 4 probes concentrated around the
                    bracketed crossing, refining the fit where it matters.
                  </span>
                </li>
              </ul>
              <p className="mt-2 text-muted-foreground/60 text-xs">
                Total: 11 probes (7 coarse + 4 dense). If all coarse probes are
                under budget, the dense pass targets the upper 30% of the coarse range.
              </p>
              <p className="mt-3 font-medium text-foreground/80">Lightness sharpening (default)</p>
              <p>
                CIE Y luminance is extracted, sharpened via unsharp mask, then RGB
                is reconstructed multiplicatively:
              </p>
              <MathBlock tex={String.raw`L = 0.2126\,R + 0.7152\,G + 0.0722\,B`} />
              <MathBlock tex={String.raw`k = \frac{L'}{L} \quad\Rightarrow\quad R' = k \cdot R, \;\; G' = k \cdot G, \;\; B' = k \cdot B`} />
              <p>
                This preserves chromaticity while modifying only perceived
                lightness — minimizing color shifts from sharpening.
              </p>
              <p className="mt-2 font-medium text-foreground/80">Unsharp mask formula</p>
              <MathBlock tex={String.raw`\text{out}(x) = \text{in}(x) + \alpha \cdot \bigl[\text{in}(x) - G_\sigma * \text{in}(x)\bigr]`} />
              <p>
                The blur <InlineMath tex="G_\sigma" /> is a separable Gaussian.
                Critically, <strong>no clamping is applied</strong> — out-of-range
                values are the artifact signal that the metric measures.
              </p>
            </PipelineStep>

            <PipelineStep n={7} title="Artifact Metric">
              <p>Two gamut metrics are available:</p>
              <ul className="mt-2 space-y-1.5 list-none">
                <li className="flex gap-2">
                  <span className="text-primary/60 mt-0.5">&#9654;</span>
                  <span>
                    <strong>Channel clipping ratio</strong> (default) — fraction of
                    individual channel values outside <InlineMath tex="[0,\,1]" />
                  </span>
                </li>
                <li className="flex gap-2">
                  <span className="text-primary/60 mt-0.5">&#9654;</span>
                  <span>
                    <strong>Pixel out-of-gamut ratio</strong> — fraction of pixels where{" "}
                    <em>any</em> channel exceeds <InlineMath tex="[0,\,1]" />
                  </span>
                </li>
              </ul>
              <p className="mt-3">
                Each probe also produces a per-component breakdown with four
                active scores:
              </p>
              <ul className="mt-2 space-y-1.5 list-none">
                {[
                  ["Gamut excursion", "drives s* selection"],
                  ["Halo ringing", "sign-change count in cross-edge profiles"],
                  ["Edge overshoot", "peak sharpening vs. gradient magnitude"],
                  ["Texture flattening", "log variance ratio in textured regions"],
                ].map(([name, desc]) => (
                  <li key={name} className="flex gap-2">
                    <span className="text-primary/60 mt-0.5">&#9654;</span>
                    <span><strong>{name}</strong> — {desc}</span>
                  </li>
                ))}
              </ul>
              <p className="mt-2 text-muted-foreground/60 text-xs">
                A weighted composite score is computed per probe for diagnostics
                but does not drive solver selection.
              </p>
            </PipelineStep>

            <PipelineStep n={8} title="Cubic Polynomial Fit">
              <p>
                The probe samples <InlineMath tex="\{(s_i,\, P_i)\}" /> are fitted to a
                cubic polynomial via <InlineMath tex="4 \times 4" /> Vandermonde normal
                equations solved by Gaussian elimination with partial pivoting:
              </p>
              <MathBlock tex={String.raw`\hat{P}(s) = a\,s^3 + b\,s^2 + c\,s + d`} />
              <p>
                All arithmetic uses double precision — the Vandermonde matrix
                has terms up to <InlineMath tex="s^6" />, and single precision causes
                catastrophic cancellation. In relative-to-base mode, a synthetic
                anchor point <InlineMath tex="(0,\,0)" /> is prepended to
                enforce <InlineMath tex="\hat{P}(0) \approx 0" />.
              </p>
              <p className="mt-2">
                Fit quality is measured by <InlineMath tex="R^2" />{" "}
                (coefficient of determination), sum of squared residuals, maximum
                absolute residual, and minimum pivot magnitude as a condition
                proxy.
              </p>
            </PipelineStep>

            <PipelineStep n={9} title="Robustness Checks">
              <p>
                Before trusting the polynomial root, multiple checks are
                performed:
              </p>
              <ul className="mt-2 space-y-1.5 list-none">
                <li className="flex gap-2">
                  <span className="text-primary/60 mt-0.5">&#9654;</span>
                  <span><strong>Monotonicity</strong> — zero or at most one inversion in probe ordering</span>
                </li>
                <li className="flex gap-2">
                  <span className="text-primary/60 mt-0.5">&#9654;</span>
                  <span><strong><InlineMath tex="R^2 > 0.85" /></strong> — fit explains at least 85% of variance</span>
                </li>
                <li className="flex gap-2">
                  <span className="text-primary/60 mt-0.5">&#9654;</span>
                  <span><strong>Condition</strong> — minimum pivot <InlineMath tex="> 10^{-8}" /></span>
                </li>
                <li className="flex gap-2">
                  <span className="text-primary/60 mt-0.5">&#9654;</span>
                  <span>
                    <strong>Leave-one-out (LOO) stability</strong> — <InlineMath tex="\max_i \left|\frac{s^*_{\text{full}} - s^*_{\text{drop}\,i}}{s^*_{\text{full}}}\right| < 0.25" />
                  </span>
                </li>
              </ul>
              <p className="mt-3">
                If any check fails, the pipeline falls back to direct search and
                records why.
              </p>
            </PipelineStep>

            <PipelineStep n={10} title="Root Solving">
              <p>
                The depressed cubic <InlineMath tex="\hat{P}(s) = P_0" /> is solved
                analytically via <strong>Cardano&apos;s formula</strong>. The
                largest real root within <InlineMath tex="[s_{\min},\, s_{\max}]" /> is
                selected — maximizing sharpness within budget.
              </p>
              <p className="mt-2">
                Four selection outcomes are possible:
              </p>
              <ul className="mt-2 space-y-1.5 list-none">
                <li className="flex gap-2">
                  <span className="text-emerald-400/70 mt-0.5">&#9679;</span>
                  <span><strong>Polynomial root</strong> — ideal: <InlineMath tex="s^*" /> from cubic solution</span>
                </li>
                <li className="flex gap-2">
                  <span className="text-amber-400/70 mt-0.5">&#9679;</span>
                  <span><strong>Best sample within budget</strong> — largest probe within <InlineMath tex="P_0" /></span>
                </li>
                <li className="flex gap-2">
                  <span className="text-orange-400/70 mt-0.5">&#9679;</span>
                  <span><strong>Least bad sample</strong> — all probes exceed budget; pick minimum metric</span>
                </li>
                <li className="flex gap-2">
                  <span className="text-red-400/70 mt-0.5">&#9679;</span>
                  <span><strong>Budget unreachable</strong> — no valid solution exists</span>
                </li>
              </ul>
            </PipelineStep>

            <PipelineStep n={11} title="Final Sharpening">
              <p>
                The selected strength <InlineMath tex="s^*" /> is applied once.
                In uniform mode this is identical to a probe step. In
                content-adaptive mode, the gain map scales strength per pixel:{" "}
                <InlineMath tex="s_{\text{eff}}(x,y) = s^* \cdot g(x,y)" />.
              </p>
              <p className="mt-2">
                <strong>Backoff loop</strong> — if the adaptive result exceeds{" "}
                <InlineMath tex="P_0" />, the global scale is multiplied by 0.8
                (configurable) for up to 4 iterations until the budget is met.
              </p>
              <p className="mt-2">
                <strong>Chroma guard</strong> (on by default) — after lightness
                sharpening, per-pixel chroma shift is measured in Cb/Cr space.
                Where the shift exceeds the threshold (25%, further tightened
                per-region), soft clamping blends back toward the original chroma.
              </p>
            </PipelineStep>

            <PipelineStep n={12} title="Quality Evaluation & Output">
              <p>
                <strong>Before</strong> final sharpening, the evaluator maps image
                content features (edge density, gradient variance) to a
                suggested maximum strength. If the solver&apos;s <InlineMath tex="s^*" /> exceeds
                this cap, it is reduced — preventing perceptual oversharpening
                that the gamut metric alone cannot detect (e.g. smooth portraits
                where gamut excursion stays low but texture damage is visible).
              </p>
              <p className="mt-2">
                <strong>After</strong> final sharpening, the evaluator also predicts
                a quality score in [0, 1] from seven image features — edge
                density, gradient variance, local variance, Laplacian variance,
                luminance entropy — for diagnostics and downstream recommendations.
              </p>
              <p className="mt-2">
                The pipeline then inspects the full diagnostics and emits
                actionable recommendations, each carrying a human-readable
                reason and a concrete parameter patch the UI can apply and
                re-run directly.
              </p>
              <p className="mt-2">
                Finally, values are clamped to <InlineMath tex="[0,\,1]" /> and the
                inverse sRGB transfer function encodes back to gamma-corrected
                8-bit output.
              </p>
            </PipelineStep>
          </div>

          {/* Mathematical foundation */}
          <SectionHeading id="math">Mathematical Foundation</SectionHeading>

          <h3 className="text-base font-heading font-semibold text-foreground mt-6 mb-2">
            The optimization problem
          </h3>
          <p className="text-sm text-muted-foreground leading-relaxed mb-4">
            Given an artifact metric <InlineMath tex="P" /> that measures the fraction of
            color values pushed outside the valid gamut by sharpening at
            strength <InlineMath tex="s" />:
          </p>
          <MathBlock tex={String.raw`\max_{s}\; s \quad \text{subject to} \quad P(s) \leq P_0`} />
          <p className="text-sm text-muted-foreground leading-relaxed">
            Rather than evaluating <InlineMath tex="P" /> at every possible <InlineMath tex="s" />{" "}
            (expensive), we sample at <InlineMath tex="N" /> probe strengths, fit a cubic,
            and solve analytically.
          </p>

          <h3 className="text-base font-heading font-semibold text-foreground mt-8 mb-2">
            Vandermonde system
          </h3>
          <p className="text-sm text-muted-foreground leading-relaxed mb-2">
            The cubic fit constructs the normal equations from the Vandermonde
            matrix:
          </p>
          <MathBlock tex={String.raw`\mathbf{A} = \begin{bmatrix} 1 & s_1 & s_1^2 & s_1^3 \\ 1 & s_2 & s_2^2 & s_2^3 \\ \vdots & \vdots & \vdots & \vdots \\ 1 & s_N & s_N^2 & s_N^3 \end{bmatrix}, \qquad \mathbf{A}^\top\!\mathbf{A}\,\mathbf{x} = \mathbf{A}^\top\!\mathbf{b}`} />
          <p className="text-sm text-muted-foreground leading-relaxed mt-3">
            <InlineMath tex="\mathbf{A}^\top\!\mathbf{A}" /> is <InlineMath tex="4 \times 4" /> with
            entries up to <InlineMath tex="\textstyle\sum s_i^6" />, requiring double precision to avoid
            catastrophic cancellation.
          </p>

          <h3 className="text-base font-heading font-semibold text-foreground mt-8 mb-2">
            Cardano&apos;s formula
          </h3>
          <p className="text-sm text-muted-foreground leading-relaxed mb-2">
            After subtracting <InlineMath tex="P_0" /> the cubic is depressed
            to <InlineMath tex="t^3 + pt + q = 0" /> and solved via the discriminant:
          </p>
          <MathBlock tex={String.raw`\Delta = -4p^3 - 27q^2`} />
          <div className="ml-1 space-y-1 text-sm text-muted-foreground mb-2">
            <p><InlineMath tex="\Delta > 0" /> — three distinct real roots (trigonometric form)</p>
            <p><InlineMath tex="\Delta = 0" /> — repeated root</p>
            <p><InlineMath tex="\Delta < 0" /> — one real root + complex conjugate pair</p>
          </div>
          <MathBlock tex={String.raw`t = \sqrt[3]{-\frac{q}{2} + \sqrt{\frac{q^2}{4} + \frac{p^3}{27}}} \;+\; \sqrt[3]{-\frac{q}{2} - \sqrt{\frac{q^2}{4} + \frac{p^3}{27}}}`} />

          {/* Design decisions */}
          <SectionHeading id="design">Key Design Decisions</SectionHeading>

          <div className="space-y-3 mt-4">
            {DESIGN_DECISIONS.map(({ title, text }) => (
              <div
                key={title}
                className="flex rounded-lg bg-card border border-border/30 overflow-hidden"
              >
                <div className="w-1 flex-shrink-0 bg-primary/20" />
                <div className="px-4 py-3.5">
                  <p className="text-sm font-medium text-foreground mb-1">{title}</p>
                  <p className="text-sm text-muted-foreground leading-relaxed">{text}</p>
                </div>
              </div>
            ))}
          </div>

          {/* Diagnostics */}
          <SectionHeading id="diagnostics">Diagnostics Output</SectionHeading>
          <p className="text-sm text-muted-foreground leading-relaxed mb-4">
            Every run produces a complete diagnostics record — a
            JSON-serializable snapshot of the entire pipeline execution:
          </p>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-px bg-border/20 rounded-lg overflow-hidden border border-border/30">
            {DIAGNOSTICS_FIELDS.map(([label, desc]) => (
              <div key={label} className="bg-card px-3.5 py-2.5">
                <p className="text-xs font-mono font-medium text-foreground/80">{label}</p>
                <p className="text-[11px] text-muted-foreground/50 mt-0.5">{desc}</p>
              </div>
            ))}
          </div>

          {/* Assumptions & open questions */}
          <SectionHeading id="assumptions">Assumptions & Open Questions</SectionHeading>
          <p className="text-sm text-muted-foreground leading-relaxed mb-4">
            The implementation is based on confirmed paper details where
            available, with documented engineering approximations elsewhere:
          </p>

          <h3 className="text-sm font-heading font-semibold text-foreground mt-6 mb-3">
            Confirmed from paper
          </h3>
          <Checklist
            marker="&#10003;"
            markerClass="text-emerald-400/60"
            items={[
              "All processing in linear RGB space",
              ["P", " = fraction of color values outside valid gamut"],
              ["P(s)", " approximated by cubic polynomial"],
              ["P_0 = 0.001", " (0.1%) as paper reference threshold"],
              "Maximize sharpness subject to artifact budget",
              ["L = 0.2126R + 0.7152G + 0.0722B", " (CIE Y luminance)"],
            ]}
          />

          <h3 className="text-sm font-heading font-semibold text-foreground mt-6 mb-3">
            Engineering approximations
          </h3>
          <Checklist
            marker="&#9679;"
            markerClass="text-amber-400/60"
            items={[
              "Lanczos3 downscale kernel (exact kernel not confirmed)",
              "Unsharp mask sharpening operator (exact operator unknown)",
              ["\\sigma = 1.0", " Gaussian default (reasonable starting value)"],
              "Percentile-stretch contrast leveling (placeholder)",
              "Two-pass probe placement calibrated on 8-scene corpus (not from paper)",
              ["P_0 = 0.003", " (Photo) and 0.001 (Precision) calibrated for two content regimes"],
              ["R^2 > 0.85", ", LOO < 25% thresholds (engineering choices)"],
            ]}
          />

          <h3 className="text-sm font-heading font-semibold text-foreground mt-6 mb-3">
            Open questions
          </h3>
          <Checklist
            marker="?"
            markerClass="text-blue-400/60"
            items={[
              "Is the sharpening operator spatial USM, frequency-domain, or something else?",
              "Does P count per-channel, per-pixel, or a custom colour-space measure?",
              "Does contrast leveling interact with the probe phase?",
              "Is the cubic fit per-channel or channel-aggregated?",
            ]}
          />

          {/* Back link */}
          <div className="mt-20 pt-8 border-t border-border/30 flex items-center justify-between">
            <Link
              to="/"
              className="flex items-center gap-1.5 text-sm font-mono text-muted-foreground hover:text-primary transition-colors"
            >
              <ArrowLeft className="h-3.5 w-3.5" />
              Back to r3sizer
            </Link>
            <span className="text-[11px] font-mono text-muted-foreground/30">
              r3sizer algorithm reference
            </span>
          </div>
        </main>
      </div>
    </div>
  );
}
