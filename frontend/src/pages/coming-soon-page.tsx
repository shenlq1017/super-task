import { Badge } from "@/components/ui/badge";
import { useFeatures } from "../providers/session-provider";
import { navTranslationKey } from "../features/registry";
import { useLocation } from "react-router-dom";
import { useTranslation } from "react-i18next";

export function ComingSoonPage() {
  const features = useFeatures();
  const loc = useLocation();
  const { t } = useTranslation();
  const feature = features.find((f) => f.path === loc.pathname);
  const key = feature ? navTranslationKey(feature.id) : null;
  const title = key ? t(key) : t("pages.coming.title");
  const since = feature?.since ?? "";

  return (
    <div className="flex flex-col items-start gap-2 p-12">
      <h1 className="m-0 text-xl font-bold tracking-tight text-[var(--t1,#222326)]">{title}</h1>
      <Badge variant="soon">{t("pages.coming.sinceBadge", { since })}</Badge>
      <p className="m-0 max-w-md text-[0.875rem] text-[var(--t2,#62666d)]">
        {t("pages.coming.desc")}
      </p>
    </div>
  );
}
