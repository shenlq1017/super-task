import { Badge } from "@/components/ui/badge";
import { useFeatures } from "../providers/session-provider";
import { navLabel } from "../features/registry";
import { useLocation } from "react-router-dom";

export function ComingSoonPage() {
  const features = useFeatures();
  const loc = useLocation();
  const feature = features.find((f) => f.path === loc.pathname);
  const title = feature ? navLabel(feature.id) : "即将推出";
  const since = feature?.since ?? "";

  return (
    <div className="flex flex-col items-start gap-2 p-12">
      <h1 className="m-0 text-xl font-bold tracking-tight text-[var(--t1,#222326)]">{title}</h1>
      <Badge variant="soon">即将 {since}</Badge>
      <p className="m-0 max-w-md text-[0.875rem] text-[var(--t2,#62666d)]">
        此能力按路线版本提供，1.0 不实现、不展示假数据。
      </p>
    </div>
  );
}
