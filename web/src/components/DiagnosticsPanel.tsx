import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useProcessorStore } from "@/stores/processor-store";
import { TimingBar } from "./TimingBar";
import { SummaryTab } from "./diagnostics/SummaryTab";
import { AdviceTab } from "./diagnostics/AdviceTab";
import { FitTab } from "./diagnostics/FitTab";
import { JsonViewer } from "./diagnostics/JsonViewer";

export function DiagnosticsPanel() {
  const diagnostics = useProcessorStore((s) => s.diagnostics);
  if (!diagnostics) return null;

  return (
    <div className="p-3">
      <Tabs defaultValue="advice" className="w-full">
        <TabsList variant="line" className="grid grid-cols-5 w-full h-8">
          <TabsTrigger value="advice" className="text-[13px] font-mono">
            Advice
          </TabsTrigger>
          <TabsTrigger value="summary" className="text-[13px] font-mono">
            Summary
          </TabsTrigger>
          <TabsTrigger value="fit" className="text-[13px] font-mono">
            Fit
          </TabsTrigger>
          <TabsTrigger value="timing" className="text-[13px] font-mono">
            Timing
          </TabsTrigger>
          <TabsTrigger value="json" className="text-[13px] font-mono">
            JSON
          </TabsTrigger>
        </TabsList>

        <TabsContent value="summary">
          <SummaryTab diagnostics={diagnostics} />
        </TabsContent>

        <TabsContent value="advice">
          <AdviceTab diagnostics={diagnostics} />
        </TabsContent>

        <TabsContent value="fit">
          <FitTab diagnostics={diagnostics} />
        </TabsContent>

        <TabsContent value="timing" className="mt-3">
          <TimingBar timing={diagnostics.timing} />
        </TabsContent>

        <TabsContent value="json" className="mt-3">
          <JsonViewer data={diagnostics} />
        </TabsContent>
      </Tabs>
    </div>
  );
}
