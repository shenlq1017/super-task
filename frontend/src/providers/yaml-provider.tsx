import { createContext, use, useEffect, useRef, useState, type ReactNode } from "react";
import { IpcFailure, type SuperTaskFile, type YamlView } from "../ipc/protocol";
import { apiYamlGet, apiYamlSaveForm, apiYamlSaveText } from "../ipc/api";
import { useWorkspace } from "./workspace-provider";

type YamlState = {
  view: YamlView | null;
  text: string;
  spec: SuperTaskFile | null;
  hash: string;
  warnings: string[];
  saving: boolean;
  error: string | null;
  lastSavedAt: number | null;
};

type YamlActions = {
  reload: () => Promise<void>;
  saveText: (text: string) => Promise<boolean>;
  saveForm: (spec: SuperTaskFile) => Promise<boolean>;
};

type YamlContextValue = { state: YamlState; actions: YamlActions };

const YamlContext = createContext<YamlContextValue | null>(null);

export function YamlProvider({ children }: { children: ReactNode }) {
  const { state: ws } = useWorkspace();
  const wsId = ws.workspaceId;
  const [view, setView] = useState<YamlView | null>(null);
  const [text, setText] = useState("");
  const [spec, setSpec] = useState<SuperTaskFile | null>(null);
  const [hash, setHash] = useState("");
  const [warnings, setWarnings] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lastSavedAt, setLastSavedAt] = useState<number | null>(null);
  const baseHashRef = useRef("");

  const reload = async () => {
    try {
      const v = await apiYamlGet();
      setView(v);
      setText(v.text);
      setSpec(v.spec);
      setHash(v.hash);
      baseHashRef.current = v.hash;
      setError(null);
    } catch (e) {
      setError(e instanceof IpcFailure ? e.message : String(e));
    }
  };

  useEffect(() => {
    if (!wsId) {
      setView(null);
      setText("");
      setSpec(null);
      setHash("");
      baseHashRef.current = "";
      return;
    }
    void reload();
  }, [wsId]);

  const saveText = async (t: string): Promise<boolean> => {
    setSaving(true);
    setError(null);
    try {
      const out = await apiYamlSaveText(t, baseHashRef.current);
      if ((out as unknown as { code?: string }).code === "YAML_CONFLICT") {
        setError("保存冲突：文件已被外部修改，请重新加载后再保存。");
        return false;
      }
      setText(out.spec ? t : t);
      setSpec(out.spec);
      setHash(out.hash);
      baseHashRef.current = out.hash;
      setWarnings(out.warnings);
      setLastSavedAt(Date.now());
      return true;
    } catch (e) {
      const msg = e instanceof IpcFailure ? e.message : String(e);
      setError(msg);
      return false;
    } finally {
      setSaving(false);
    }
  };

  const saveForm = async (s: SuperTaskFile): Promise<boolean> => {
    setSaving(true);
    setError(null);
    try {
      const out = await apiYamlSaveForm(s, baseHashRef.current);
      setSpec(out.spec);
      setHash(out.hash);
      baseHashRef.current = out.hash;
      setWarnings(out.warnings);
      setLastSavedAt(Date.now());
      return true;
    } catch (e) {
      setError(e instanceof IpcFailure ? e.message : String(e));
      return false;
    } finally {
      setSaving(false);
    }
  };

  const value: YamlContextValue = {
    state: { view, text, spec, hash, warnings, saving, error, lastSavedAt },
    actions: { reload, saveText, saveForm },
  };

  return <YamlContext value={value}>{children}</YamlContext>;
}

export function useYaml(): YamlContextValue {
  const ctx = use(YamlContext);
  if (!ctx) throw new Error("useYaml 必须在 YamlProvider 内");
  return ctx;
}
