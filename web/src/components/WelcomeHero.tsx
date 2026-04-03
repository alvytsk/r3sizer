import { Lock, Pipette, Scaling, Sparkles, Target } from "lucide-react";
import { LogoMark } from "./LogoMark";
import { ImageUpload } from "./ImageUpload";
import BlurText from "./BlurText";
import ShinyText from "./ShinyText";

export function WelcomeHero() {
  return (
    <div className="flex-1 flex items-center justify-center px-6">
      <div className="flex flex-col items-center gap-8 max-w-lg w-full -mt-10">
        {/* Hero logo + title */}
        <div className="flex flex-col items-center gap-4 text-center">
          <div className="animate-fade-up glow-amber rounded-2xl p-2">
            <LogoMark className="h-16 w-16 text-primary" />
          </div>
          <div className="flex flex-col items-center gap-2.5 animate-fade-up delay-100">
            <h2 className="font-mono text-3xl font-bold tracking-tight text-primary glow-amber-text">
              r3sizer
            </h2>
            <BlurText
              text="Precision downscaling with automatic sharpness optimization."
              className="text-sm text-muted-foreground max-w-xs justify-center"
              animateBy="words"
              direction="bottom"
              delay={80}
              stepDuration={0.4}
              animationFrom={{ filter: 'blur(12px)', opacity: 0, y: 12 }}
              animationTo={[
                { filter: 'blur(4px)', opacity: 0.4, y: -2 },
                { filter: 'blur(0px)', opacity: 1, y: 0 }
              ]}
              easing={[0.16, 1, 0.3, 1]}
            />
            <span className="inline-flex items-center gap-1.5 px-3 py-1 rounded-full border border-green-500/25 bg-green-500/[0.06] text-xs font-mono tracking-wide">
              <Lock className="h-3 w-3 text-green-400/80" />
              <ShinyText
                text="Runs entirely in your browser"
                speed={3.5}
                delay={1}
                color="oklch(0.72 0.15 145)"
                shineColor="oklch(0.90 0.20 145)"
                className="text-xs font-mono tracking-wide"
              />
            </span>
          </div>
        </div>
        {/* Upload zone with crop marks */}
        <div className="relative w-full animate-fade-up delay-300">
          {/* Corner crop marks */}
          <div className="absolute -top-2 -left-2 w-5 h-5 border-t-2 border-l-2 border-primary/40 rounded-tl-sm" />
          <div className="absolute -top-2 -right-2 w-5 h-5 border-t-2 border-r-2 border-primary/40 rounded-tr-sm" />
          <div className="absolute -bottom-2 -left-2 w-5 h-5 border-b-2 border-l-2 border-primary/40 rounded-bl-sm" />
          <div className="absolute -bottom-2 -right-2 w-5 h-5 border-b-2 border-r-2 border-primary/40 rounded-br-sm" />
          <ImageUpload />
        </div>
        {/* Pipeline hint — hidden on narrow screens */}
        <div className="hidden sm:flex items-center gap-4 text-[11px] font-mono text-muted-foreground/50 animate-fade-up delay-400">
          <span className="inline-flex items-center gap-1"><Pipette className="h-3 w-3" />linearize</span>
          <span className="text-primary/30">&rarr;</span>
          <span className="inline-flex items-center gap-1"><Scaling className="h-3 w-3" />downscale</span>
          <span className="text-primary/30">&rarr;</span>
          <span className="inline-flex items-center gap-1"><Sparkles className="h-3 w-3" />sharpen</span>
          <span className="text-primary/30">&rarr;</span>
          <span className="inline-flex items-center gap-1"><Target className="h-3 w-3" />optimize</span>
        </div>
      </div>
    </div>
  );
}
