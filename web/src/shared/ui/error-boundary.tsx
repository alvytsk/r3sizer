import { Component, type ReactNode } from "react";
import { AlertTriangle } from "lucide-react";

interface Props {
  /** Label shown in the fallback UI to identify which panel crashed. */
  panel: string;
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/**
 * Catches rendering errors in a subtree and shows a compact fallback
 * instead of crashing the entire app. Used around panels that render
 * external data (WASM diagnostics, parameters).
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  render() {
    if (this.state.error) {
      return (
        <div className="flex flex-col items-center justify-center gap-3 px-6 py-16 text-center">
          <div className="rounded-full p-3 bg-destructive/10 text-destructive/60">
            <AlertTriangle className="h-5 w-5" />
          </div>
          <div className="space-y-1">
            <p className="text-sm text-destructive/80">
              {this.props.panel} crashed
            </p>
            <p className="text-[11px] font-mono text-muted-foreground/50 max-w-[260px] break-words">
              {this.state.error.message}
            </p>
          </div>
          <button
            onClick={() => this.setState({ error: null })}
            className="text-[11px] font-mono text-primary hover:text-primary/80 transition-colors"
          >
            Try again
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}
