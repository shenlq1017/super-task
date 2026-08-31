import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { KeyRound, RefreshCw, Search, ShieldCheck, Trash2, UserPlus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { useToast } from "@/components/ui/toast";
import { useAuth } from "@/providers/auth";
import * as api from "@/lib/api";
import { ApiError, type AccountRow, type Role } from "@/lib/api";
import { t } from "@/lib/labels";
import { cn } from "@/lib/utils";
import { CreateAccountDialog } from "@/components/create-account-dialog";
import { PasswordDialog } from "@/components/password-dialog";

type PendingAction = {
  kind: "disable" | "enable" | "promote" | "demote" | "delete";
  row: AccountRow;
};

function messageFor(error: unknown): string {
  return error instanceof ApiError ? error.message || error.code : String(error);
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KiB", "MiB", "GiB"];
  let value = bytes / 1024;
  let index = 0;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[index]}`;
}

function formatTime(epochSeconds: number): string {
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(
    new Date(epochSeconds * 1000),
  );
}

export function AccountsPage() {
  const { toast } = useToast();
  const { session } = useAuth();
  const [rows, setRows] = useState<AccountRow[]>([]);
  const [query, setQuery] = useState("");
  const [state, setState] = useState<"idle" | "loading" | "error">("loading");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [pending, setPending] = useState<PendingAction | null>(null);
  const [creating, setCreating] = useState(false);
  const [passwordTarget, setPasswordTarget] = useState<AccountRow | null>(null);

  const load = useCallback(async (search: string) => {
    setState((current) => (current === "idle" ? "loading" : current));
    setLoadError(null);
    try {
      setRows(await api.listAccounts({ query: search.trim() || undefined }));
      setState("idle");
    } catch (error) {
      setLoadError(messageFor(error));
      setState("error");
    }
  }, []);

  // Debounced so typing an email filters without a request per keystroke; the first
  // load fires immediately so the table does not sit on a spinner after login.
  const firstLoad = useRef(true);
  useEffect(() => {
    const delay = firstLoad.current ? 0 : 250;
    firstLoad.current = false;
    const timer = setTimeout(() => void load(query), delay);
    return () => clearTimeout(timer);
  }, [load, query]);

  const replaceRow = useCallback((next: AccountRow) => {
    setRows((current) => current.map((row) => (row.id === next.id ? next : row)));
  }, []);

  const runPending = useCallback(async () => {
    if (!pending) return;
    setBusyId(pending.row.id);
    const label = pending.row.email;
    try {
      switch (pending.kind) {
        case "disable":
        case "enable": {
          const next = await api.setDisabled(pending.row.id, pending.kind === "disable");
          replaceRow(next);
          toast(t(pending.kind === "disable" ? "done.disabled" : "done.enabled", { email: label }), pending.kind === "disable" ? "warn" : "ok");
          break;
        }
        case "promote":
        case "demote": {
          const role: Role = pending.kind === "promote" ? "admin" : "user";
          const next = await api.setRole(pending.row.id, role);
          replaceRow(next);
          toast(t("done.roleSet", { email: label }), "ok");
          break;
        }
        case "delete": {
          await api.deleteAccount(pending.row.id);
          setRows((current) => current.filter((row) => row.id !== pending.row.id));
          toast(t("done.deleted", { email: label }), "ok");
          break;
        }
      }
      setPending(null);
    } catch (error) {
      toast(messageFor(error), "err");
    } finally {
      setBusyId(null);
    }
  }, [pending, replaceRow, toast]);

  const total = useMemo(
    () => ({ entities: rows.reduce((sum, row) => sum + row.entity_count, 0), bytes: rows.reduce((sum, row) => sum + row.entity_bytes, 0) }),
    [rows],
  );

  const busy = state === "loading" || busyId !== null;

  return (
    <div className="p-6">
      <div className="mx-auto flex max-w-5xl flex-col gap-4">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 className="text-[1.05rem] font-bold tracking-tight text-[var(--t1,#222326)]">{t("accounts.title")}</h2>
            <p className="mt-0.5 text-[0.78rem] text-[var(--t3,#8a8f98)]">{t("accounts.subtitle")}</p>
          </div>
          <div className="flex items-center gap-2">
            <Button variant="soft" size="sm" className="gap-1" onClick={() => void load(query)} disabled={busy} aria-label={t("common.refresh")}>
              <RefreshCw className={cn("size-3.5", state === "loading" && "animate-spin")} /> {t("common.refresh")}
            </Button>
            <Button size="sm" className="gap-1" onClick={() => setCreating(true)} disabled={busy}>
              <UserPlus className="size-3.5" /> {t("accounts.new")}
            </Button>
          </div>
        </div>

        <Card className="overflow-hidden">
          <div className="flex flex-wrap items-center gap-2 border-b border-[var(--line,#e6e6e6)] p-3">
            <div className="relative min-w-[14rem] flex-1">
              <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-[var(--t3,#8a8f98)]" />
              <Input
                className="pl-8"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder={t("accounts.searchPlaceholder")}
                type="search"
                aria-label={t("accounts.searchPlaceholder")}
              />
            </div>
            <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">
              {t("accounts.count", { n: rows.length })} · {t("accounts.usage", { entities: total.entities, bytes: formatBytes(total.bytes) })}
            </span>
          </div>

          {state === "error" ? (
            <div className="flex flex-wrap items-center gap-3 p-6">
              <p className="text-[0.8rem] text-[var(--st-danger,#dc2626)]">{loadError ?? t("accounts.loadFailed")}</p>
              <Button variant="outline" size="sm" onClick={() => void load(query)}>{t("common.retry")}</Button>
            </div>
          ) : !rows.length ? (
            <p className="p-6 text-[0.8rem] text-[var(--t3,#8a8f98)]">
              {state === "loading" ? t("common.loading") : t("accounts.empty")}
            </p>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full border-collapse text-[0.78rem]">
                <thead>
                  <tr className="border-b border-[var(--line,#e6e6e6)] text-left text-[0.72rem] text-[var(--t3,#8a8f98)]">
                    <th className="px-3 py-2 font-medium">{t("col.email")}</th>
                    <th className="px-3 py-2 font-medium">{t("col.role")}</th>
                    <th className="px-3 py-2 font-medium">{t("col.status")}</th>
                    <th className="px-3 py-2 text-right font-medium">{t("col.usage")}</th>
                    <th className="px-3 py-2 font-medium">{t("col.created")}</th>
                    <th className="px-3 py-2 text-right font-medium">{t("col.actions")}</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((row) => (
                    <AccountRowView
                      key={row.id}
                      row={row}
                      isSelf={row.id === session?.accountId}
                      busy={busy}
                      onAction={(kind, target) => setPending({ kind, row: target })}
                      onPassword={(target) => setPasswordTarget(target)}
                    />
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Card>
      </div>

      <ConfirmDialog
        open={pending !== null}
        title={t(`confirm.${pending?.kind}.title`)}
        description={t(`confirm.${pending?.kind}.body`, {
          email: pending?.row.email ?? "",
          entities: String(pending?.row.entity_count ?? 0),
        })}
        confirmText={pending ? t(`action.${pending.kind}`) : undefined}
        destructive={pending?.kind === "delete"}
        busy={busyId !== null}
        onConfirm={() => void runPending()}
        onCancel={() => setPending(null)}
      />

      <CreateAccountDialog
        open={creating}
        busy={busy}
        onClose={() => setCreating(false)}
        onCreated={(row) => {
          setRows((current) => [row, ...current.filter((item) => item.id !== row.id)]);
          setCreating(false);
          toast(t("create.created", { email: row.email }), "ok");
        }}
        onError={(message) => toast(message, "err")}
      />

      <PasswordDialog
        row={passwordTarget}
        busy={busy}
        onClose={() => setPasswordTarget(null)}
        onSaved={() => {
          toast(t("password.saved", { email: passwordTarget?.email ?? "" }), "ok");
          setPasswordTarget(null);
        }}
        onError={(message) => toast(message, "err")}
      />
    </div>
  );
}

function AccountRowView({
  row,
  isSelf,
  busy,
  onAction,
  onPassword,
}: {
  row: AccountRow;
  isSelf: boolean;
  busy: boolean;
  onAction: (kind: PendingAction["kind"], row: AccountRow) => void;
  onPassword: (row: AccountRow) => void;
}) {
  const selfHint = t("accounts.selfHint");
  return (
    <tr className="border-b border-[var(--line-soft,#eff0f2)] last:border-b-0 hover:bg-[var(--surface-2,#f3f4f5)]">
      <td className="px-3 py-2 font-mono text-[0.75rem] text-[var(--t1,#222326)]">
        {row.email}
        {isSelf ? <span className="ml-1.5 text-[0.66rem] text-[var(--t3,#8a8f98)]">{t("accounts.self")}</span> : null}
      </td>
      <td className="px-3 py-2">
        {row.role === "admin" ? (
          <span className="inline-flex items-center gap-1 text-[var(--st-accent,#5e6ad2)]">
            <ShieldCheck className="size-3.5" /> {t("role.admin")}
          </span>
        ) : (
          <span className="text-[var(--t2,#62666d)]">{t("role.user")}</span>
        )}
      </td>
      <td className="px-3 py-2">
        <Badge variant={row.disabled ? "destructive" : "outline"}>
          {t(row.disabled ? "state.disabled" : "state.enabled")}
        </Badge>
      </td>
      <td className="px-3 py-2 text-right font-mono text-[0.72rem] text-[var(--t2,#62666d)]">
        {row.entity_count} · {formatBytes(row.entity_bytes)}
      </td>
      <td className="px-3 py-2 text-[0.72rem] text-[var(--t3,#8a8f98)]">{formatTime(row.created_at)}</td>
      <td className="px-3 py-2">
        <div className="flex flex-wrap items-center justify-end gap-1">
          <Button variant="outline" size="xs" className="gap-1" disabled={busy} onClick={() => onPassword(row)}>
            <KeyRound className="size-3" /> {t("action.setPassword")}
          </Button>
          {row.role === "admin" ? (
            <Button
              variant="warn"
              size="xs"
              disabled={busy || isSelf}
              title={isSelf ? selfHint : undefined}
              onClick={() => onAction("demote", row)}
            >
              {t("action.demote")}
            </Button>
          ) : (
            <Button variant="soft" size="xs" disabled={busy} onClick={() => onAction("promote", row)}>
              {t("action.promote")}
            </Button>
          )}
          {row.disabled ? (
            <Button variant="success" size="xs" disabled={busy} onClick={() => onAction("enable", row)}>
              {t("action.enable")}
            </Button>
          ) : (
            <Button
              variant="warn"
              size="xs"
              disabled={busy || isSelf}
              title={isSelf ? selfHint : undefined}
              onClick={() => onAction("disable", row)}
            >
              {t("action.disable")}
            </Button>
          )}
          <Button
            variant="destructive"
            size="icon-xs"
            disabled={busy || isSelf}
            title={isSelf ? selfHint : t("action.delete")}
            aria-label={t("action.delete")}
            onClick={() => onAction("delete", row)}
          >
            <Trash2 className="size-3" />
          </Button>
        </div>
      </td>
    </tr>
  );
}
