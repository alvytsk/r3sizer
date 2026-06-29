import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/shared/ui/tabs";
import { useTranslation } from "react-i18next";
import { useOutputStore } from "@/entities/output";
import { TimingBar } from "./timing-bar";
import { SummaryTab } from "./summary-tab";
import { AdviceTab } from "./advice-tab";
import { FitTab } from "./fit-tab";
import { JsonViewer } from "./json-viewer";

export function DiagnosticsPanel() {
  const { t } = useTranslation();
  const diagnostics = useOutputStore((s) => s.diagnostics);
  if (!diagnostics) return null;

  return (
    <div className="p-3">
      <Tabs defaultValue="advice" className="w-full">
        <TabsList variant="line" className="grid grid-cols-5 w-full h-8">
          <TabsTrigger value="advice" className="text-[13px] font-mono">
            {t("diagnostics.advice")}
          </TabsTrigger>
          <TabsTrigger value="summary" className="text-[13px] font-mono">
            {t("diagnostics.summary")}
          </TabsTrigger>
          <TabsTrigger value="fit" className="text-[13px] font-mono">
            {t("diagnostics.fit")}
          </TabsTrigger>
          <TabsTrigger value="timing" className="text-[13px] font-mono">
            {t("diagnostics.timing")}
          </TabsTrigger>
          <TabsTrigger value="json" className="text-[13px] font-mono">
            {t("diagnostics.json")}
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
