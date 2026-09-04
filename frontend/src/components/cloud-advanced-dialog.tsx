import { Settings } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useTranslation } from "react-i18next";

export function CloudAdvancedDialog(props: {
  open: boolean;
  busy: boolean;
  endpoint: string;
  endpointError: string | null;
  telemetryEnabled: boolean;
  onEndpointChange: (value: string) => void;
  onSaveEndpoint: (event: React.FormEvent<HTMLFormElement>) => void;
  onTelemetryChange: (enabled: boolean) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Dialog open={props.open} onOpenChange={(open) => (!open ? props.onClose() : undefined)}>
      <DialogContent className="gap-0 overflow-hidden p-0 sm:max-w-lg">
        <DialogHeader className="gap-1 border-b border-[var(--line)] px-5 py-4">
          <DialogTitle className="flex items-center gap-2">
            <Settings className="size-4 text-[var(--st-accent)]" />
            {t("pages.cloud.advancedTitle")}
          </DialogTitle>
          <DialogDescription className="text-[0.75rem]">{t("pages.cloud.advancedHint")}</DialogDescription>
        </DialogHeader>
        <form onSubmit={props.onSaveEndpoint} className="px-5 py-4">
          <label htmlFor="cloud-endpoint" className="text-[0.75rem] text-[var(--t3)]">{t("pages.cloud.endpoint")}</label>
          <div className="mt-1 flex flex-col gap-2 sm:flex-row">
            <Input
              id="cloud-endpoint"
              value={props.endpoint}
              onChange={(event) => props.onEndpointChange(event.target.value)}
              type="url"
              inputMode="url"
              aria-invalid={!!props.endpointError}
              spellCheck={false}
            />
            <Button variant="success" size="sm" type="submit" disabled={props.busy || !props.endpoint.trim()}>
              {t("pages.cloud.saveEndpoint")}
            </Button>
          </div>
          <p className="mt-1 text-[0.72rem] text-[var(--t3)]">{t("pages.cloud.endpointLocalHint")}</p>
          {props.endpointError ? <p className="mt-1 text-[0.72rem] text-[#DC2626]" role="alert">{props.endpointError}</p> : null}
          <div className="mt-4 border-t border-[var(--line)] pt-3">
            <label className="flex cursor-pointer items-start justify-between gap-4 text-[0.8rem] text-[var(--t1)]">
              <span>{t("pages.cloud.telemetry")}</span>
              <input
                type="checkbox"
                checked={props.telemetryEnabled}
                onChange={(event) => props.onTelemetryChange(event.target.checked)}
                disabled={props.busy}
                aria-label={t("pages.cloud.telemetry")}
              />
            </label>
            <p className="mt-1 text-[0.72rem] text-[var(--t3)]">{t("pages.cloud.telemetryHint")}</p>
          </div>
          <DialogFooter className="mx-0 mb-0 mt-4 px-0 sm:justify-end">
            <Button variant="outline" size="sm" type="button" onClick={props.onClose}>{t("common.close")}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
