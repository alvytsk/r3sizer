import { Link } from "@tanstack/react-router";
import { ArrowLeft, ArrowRight } from "lucide-react";
import katex from "katex";
import "katex/dist/katex.min.css";

/* ---------- content data ---------- */

const PIPELINE_STAGES = [
  "linearize", "downscale", "contrast", "baseline", "probe",
  "fit", "solve", "sharpen", "clamp", "encode",
] as const;

const DESIGN_DECISIONS = [
  {
    title: "f32 pixels, f64 fitting",
    text: "Image data uses f32 for memory efficiency. Polynomial fitting uses f64 because the Vandermonde normal equations have terms up to s\u2076 \u2014 f32 causes catastrophic cancellation in the 4\u00d74 system.",
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
    text: "RelativeToBase mode subtracts pre-sharpen artifacts from each measurement. This isolates sharpening-induced artifacts from those inherent in the downscale, producing a cleaner fit.",
  },
  {
    title: "Fallback is not an error",
    text: "When the cubic solve finds no root in range, the pipeline falls back to the best probe sample. It always produces a result. The selection outcome is reported via typed enums for full transparency.",
  },
  {
    title: "Immutable base image",
    text: "The downscaled image is never mutated during probing. Each probe produces a fresh allocation, ensuring the final sharpening pass uses the exact same base as the probes.",
  },
] as const;

const DIAGNOSTICS_FIELDS = [
  ["Input/Output sizes", "Original and target dimensions"],
  ["Probe samples", "All (strength, metric) pairs measured"],
  ["Fit coefficients", "The cubic polynomial a, b, c, d"],
  ["Fit quality", "R\u00b2, residuals, condition number"],
  ["Robustness flags", "Monotonicity, LOO stability"],
  ["Selection mode", "How s* was chosen"],
  ["Fallback reason", "Why polynomial root was bypassed"],
  ["Per-stage timing", "Microsecond wall-clock per stage"],
  ["Metric breakdown", "Component scores per probe"],
  ["Crossing status", "Where \u0050\u0302(s) intersects P\u2080"],
] as const;

/* ---------- KaTeX helpers ---------- */

function renderTex(tex: string, displayMode = false) {
  return katex.renderToString(tex, { throwOnError: false, displayMode });
}

function InlineMath({ tex }: { tex: string }) {
  return <span dangerouslySetInnerHTML={{ __html: renderTex(tex) }} />;
}

function MathBlock({ tex }: { tex: string }) {
  return (
    <div className="my-4 px-5 py-3.5 rounded-lg bg-surface border border-border/40 overflow-x-auto">
      <div dangerouslySetInnerHTML={{ __html: renderTex(tex, true) }} />
    </div>
  );
}

/* ---------- tiny reusable pieces ---------- */

function Tag({ children }: { children: React.ReactNode }) {
  return (
    <span className="inline-block px-2 py-0.5 rounded-md bg-primary/10 text-primary text-[11px] font-mono tracking-wide">
      {children}
    </span>
  );
}

function StageBadge({ n }: { n: number }) {
  return (
    <span className="flex-shrink-0 inline-flex items-center justify-center w-7 h-7 rounded-full bg-primary/15 text-primary text-xs font-mono font-bold border border-primary/20">
      {n}
    </span>
  );
}

function Mono({ children }: { children: React.ReactNode }) {
  return <code className="text-primary/90 bg-surface px-1.5 py-0.5 rounded text-[13px]">{children}</code>;
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
    <h2 id={id} className="text-xl font-heading font-bold text-foreground mt-16 mb-4 flex items-center gap-3 scroll-mt-24">
      <span className="w-8 h-px bg-primary/40" />
      {children}
    </h2>
  );
}

function PipelineStep({
  n,
  title,
  module,
  children,
}: {
  n: number;
  title: string;
  module: string;
  children: React.ReactNode;
}) {
  return (
    <div className="group relative flex gap-4">
      <div className="flex flex-col items-center">
        <StageBadge n={n} />
        <div className="flex-1 w-px bg-border/30 group-last:hidden mt-2" />
      </div>
      <div className="pb-8 min-w-0">
        <div className="flex items-baseline gap-2 mb-1.5">
          <h3 className="text-base font-heading font-semibold text-foreground">{title}</h3>
          <span className="text-[11px] font-mono text-muted-foreground/50">{module}</span>
        </div>
        <div className="text-sm text-muted-foreground leading-relaxed">{children}</div>
      </div>
    </div>
  );
}

/* ---------- main page ---------- */

export default function AlgorithmPage() {
  return (
    <div className="min-h-screen bg-background grain relative">
      {/* Navigation bar */}
      <nav className="sticky top-0 z-20 border-b border-border/60 backdrop-blur-sm bg-background/80">
        <div className="max-w-3xl mx-auto px-6 py-2.5 flex items-center gap-3">
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

      {/* Content */}
      <main className="max-w-3xl mx-auto px-6 pt-12 pb-24">
        {/* Hero */}
        <header className="mb-16 animate-fade-up">
          <div className="flex items-center gap-3 mb-4">
            <Tag>v0.1</Tag>
            <Tag>auto-sharpness</Tag>
          </div>
          <h1 className="text-3xl sm:text-4xl font-heading font-bold text-foreground tracking-tight mb-4">
            The Algorithm
          </h1>
          <p className="text-base text-muted-foreground leading-relaxed max-w-xl">
            r3sizer automatically selects the optimal sharpening strength when
            downscaling images. It fits a cubic polynomial to measured artifact
            ratios across probe strengths, then solves for the maximum sharpness
            that stays within a perceptual quality budget.
          </p>
        </header>

        {/* Core idea */}
        <section className="mb-12 animate-fade-up delay-100">
          <div className="rounded-xl border border-primary/20 bg-primary/[0.04] px-6 py-5">
            <p className="text-sm text-primary/90 leading-relaxed">
              <span className="font-bold font-mono">Core constraint:</span>{" "}
              select sharpening strength <InlineMath tex="s^*" /> that maximizes
              perceptual sharpness while keeping the artifact
              ratio <InlineMath tex="P(s^*) \leq P_0" />, where <InlineMath tex="P_0" /> defaults
              to <Mono>0.001</Mono> (0.1% of color values outside the valid gamut).
            </p>
          </div>
        </section>

        {/* Pipeline overview — compact visual */}
        <SectionHeading id="pipeline">Pipeline Overview</SectionHeading>
        <p className="text-sm text-muted-foreground mb-6">
          Every image passes through a fixed sequence of stages. All processing
          happens in linear RGB (f32) — sRGB encoding is only applied at the
          very end.
        </p>

        <div className="rounded-xl border border-border/40 bg-card px-5 py-6 mb-8">
          <div className="flex flex-wrap items-center gap-2 text-xs font-mono">
            {PIPELINE_STAGES.map((stage, i, arr) => (
              <span key={stage} className="flex items-center gap-2">
                <span className="px-2.5 py-1 rounded-md bg-surface border border-border/40 text-foreground/80 whitespace-nowrap">
                  {stage}
                </span>
                {i < arr.length - 1 && (
                  <ArrowRight className="h-3 w-3 text-primary/40 flex-shrink-0" />
                )}
              </span>
            ))}
          </div>
        </div>

        {/* Stage-by-stage breakdown */}
        <SectionHeading id="stages">Stage Details</SectionHeading>

        <div className="mt-6">
          <PipelineStep n={1} title="Input Decoding & Linearization" module="color.rs">
            <p>
              The input image is decoded via the <Mono>image</Mono> crate and
              normalized to f32 in <InlineMath tex="[0,\,1]" />. The IEC 61966-2-1 (sRGB)
              transfer function is applied immediately to convert to linear
              light:
            </p>
            <MathBlock tex={String.raw`f(v) = \begin{cases} \dfrac{v}{12.92} & v \leq 0.04045 \\[6pt] \left(\dfrac{v + 0.055}{1.055}\right)^{2.4} & v > 0.04045 \end{cases}`} />
            <p>
              All subsequent processing operates in this linear space where
              physical light intensity is proportional to pixel value.
            </p>
          </PipelineStep>

          <PipelineStep n={2} title="Downscale" module="resize.rs">
            <p>
              Lanczos3 resampling reduces the image to the target dimensions.
              The kernel is applied in linear light — no gamma curve distortion.
              Output remains unclamped f32 so the full dynamic range is
              preserved.
            </p>
          </PipelineStep>

          <PipelineStep n={3} title="Contrast Leveling" module="contrast.rs">
            <p className="italic text-muted-foreground/60">
              Optional stage (disabled by default). Applies per-channel 1st–99th
              percentile stretch. This is a documented placeholder — the exact
              formula from the paper is not yet known.
            </p>
          </PipelineStep>

          <PipelineStep n={4} title="Baseline Measurement" module="metrics.rs">
            <p>
              Before any sharpening, the artifact ratio of the downscaled base
              image is measured. In <Mono>RelativeToBase</Mono> mode (default),
              this baseline is subtracted from each probe measurement so the
              fitted polynomial only reflects <em>sharpening-induced</em>{" "}
              artifacts, not resize artifacts.
            </p>
          </PipelineStep>

          <PipelineStep n={5} title="Probe Sharpening" module="sharpen.rs + color.rs">
            <p>
              The core exploration phase. For each strength in the probe set
              (default: 7 non-uniform samples denser near zero), sharpening is
              applied and artifacts measured.
            </p>
            <div className="mt-3 mb-2 text-xs font-mono text-muted-foreground/70">
              Default probes: [0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0]
            </div>
            <p className="mt-2 font-medium text-foreground/80">Lightness sharpening (default)</p>
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
              The blur <InlineMath tex="G_\sigma" /> is a hand-rolled separable Gaussian.
              Critically, <strong>no clamping is applied</strong> — out-of-range
              values are the artifact signal that the metric measures.
            </p>
          </PipelineStep>

          <PipelineStep n={6} title="Artifact Metric" module="metrics.rs">
            <p>Two metrics are implemented, selectable via <Mono>ArtifactMetric</Mono>:</p>
            <ul className="mt-2 space-y-1.5 list-none">
              <li className="flex gap-2">
                <span className="text-primary/60 mt-0.5">&#9654;</span>
                <span>
                  <Mono>ChannelClippingRatio</Mono> (default) — fraction of
                  individual channel values outside <InlineMath tex="[0,\,1]" />
                </span>
              </li>
              <li className="flex gap-2">
                <span className="text-primary/60 mt-0.5">&#9654;</span>
                <span>
                  <Mono>PixelOutOfGamutRatio</Mono> — fraction of pixels where{" "}
                  <em>any</em> channel exceeds <InlineMath tex="[0,\,1]" />
                </span>
              </li>
            </ul>
            <p className="mt-3">
              Each probe also produces a <Mono>MetricBreakdown</Mono> with
              component-wise scores (GamutExcursion active in v0.1; HaloRinging,
              EdgeOvershoot, TextureFlattening are scaffolded for v0.2).
            </p>
          </PipelineStep>

          <PipelineStep n={7} title="Cubic Polynomial Fit" module="fit.rs">
            <p>
              The probe samples <InlineMath tex="\{(s_i,\, P_i)\}" /> are fitted to a
              cubic polynomial via <InlineMath tex="4 \times 4" /> Vandermonde normal
              equations solved by Gaussian elimination with partial pivoting:
            </p>
            <MathBlock tex={String.raw`\hat{P}(s) = a\,s^3 + b\,s^2 + c\,s + d`} />
            <p>
              All arithmetic is in <strong>f64</strong> — the Vandermonde matrix
              has terms up to <InlineMath tex="s^6" />, and f32 causes catastrophic
              cancellation. In <Mono>RelativeToBase</Mono> mode, a synthetic
              anchor point <InlineMath tex="(0,\,0)" /> is prepended to
              enforce <InlineMath tex="\hat{P}(0) \approx 0" />.
            </p>
            <p className="mt-2">
              Fit quality is reported as <Mono>FitQuality</Mono>: <InlineMath tex="R^2" />{" "}
              (coefficient of determination), sum of squared residuals, maximum
              absolute residual, and minimum pivot magnitude (condition proxy).
            </p>
          </PipelineStep>

          <PipelineStep n={8} title="Robustness Checks" module="pipeline.rs">
            <p>
              Before trusting the polynomial root, multiple checks are
              performed and recorded in <Mono>RobustnessFlags</Mono>:
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
                  <strong>Leave-one-out (LOO) stability</strong> — <InlineMath tex="\max_i \left|\frac{s^*_{\text{full}} - s^*_{\text{drop}\,i}}{s^*_{\text{full}}}\right| < 0.5" />
                </span>
              </li>
            </ul>
            <p className="mt-3">
              If any check fails, the pipeline falls back to direct search and
              records a typed <Mono>FallbackReason</Mono>.
            </p>
          </PipelineStep>

          <PipelineStep n={9} title="Root Solving" module="solve.rs">
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
                <span><Mono>PolynomialRoot</Mono> — ideal: <InlineMath tex="s^*" /> from cubic solution</span>
              </li>
              <li className="flex gap-2">
                <span className="text-amber-400/70 mt-0.5">&#9679;</span>
                <span><Mono>BestSampleWithinBudget</Mono> — largest probe within <InlineMath tex="P_0" /></span>
              </li>
              <li className="flex gap-2">
                <span className="text-orange-400/70 mt-0.5">&#9679;</span>
                <span><Mono>LeastBadSample</Mono> — all probes exceed budget; pick minimum metric</span>
              </li>
              <li className="flex gap-2">
                <span className="text-red-400/70 mt-0.5">&#9679;</span>
                <span><Mono>BudgetUnreachable</Mono> — no valid solution exists</span>
              </li>
            </ul>
          </PipelineStep>

          <PipelineStep n={10} title="Final Sharpening & Output" module="pipeline.rs + color.rs">
            <p>
              The selected strength <InlineMath tex="s^*" /> is applied once to produce
              the final sharpened image. Values are clamped to <InlineMath tex="[0,\,1]" />{" "}
              (or optionally normalized), then the inverse sRGB transfer function
              encodes back to gamma-corrected u8 output.
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
          entries up to <InlineMath tex="\textstyle\sum s_i^6" />, requiring f64 to avoid
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

        <div className="space-y-4 mt-4">
          {DESIGN_DECISIONS.map(({ title, text }) => (
            <div
              key={title}
              className="flex gap-4 p-4 rounded-lg bg-card border border-border/30"
            >
              <span className="text-primary/50 text-lg mt-0.5 flex-shrink-0">&#9670;</span>
              <div>
                <p className="text-sm font-medium text-foreground mb-1">{title}</p>
                <p className="text-sm text-muted-foreground leading-relaxed">{text}</p>
              </div>
            </div>
          ))}
        </div>

        {/* Diagnostics */}
        <SectionHeading id="diagnostics">Diagnostics Output</SectionHeading>
        <p className="text-sm text-muted-foreground leading-relaxed mb-4">
          Every run produces a complete <Mono>AutoSharpDiagnostics</Mono>{" "}
          record — a JSON-serializable snapshot of the entire pipeline
          execution:
        </p>
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          {DIAGNOSTICS_FIELDS.map(([label, desc]) => (
            <div
              key={label}
              className="px-3.5 py-2.5 rounded-md bg-surface border border-border/30"
            >
              <p className="text-xs font-mono font-medium text-foreground/80">{label}</p>
              <p className="text-[11px] text-muted-foreground/60 mt-0.5">{desc}</p>
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
            ["P_0 = 0.001", " (0.1%) as target threshold"],
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
            "Probe strengths chosen empirically, not from paper",
            ["R^2 > 0.85", ", LOO < 50% thresholds (engineering choices)"],
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
  );
}
