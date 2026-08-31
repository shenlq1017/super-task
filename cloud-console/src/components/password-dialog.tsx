import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import * as api from "@/lib/api";
import { ApiError } from "@/lib/api";
import { MIN_PASSWORD_CHARS, t } from "@/lib/labels";

/** Admin-set password: the server never echoes the credential back in any response. */
export function PasswordDialog({
  row,
  busy,
  onClose,
  onSaved,
  onError,
}: {
  row: api.AccountRow | null;
  busy: boolean;
  onClose: () => void;
  onSaved: () => void;
  onError: (message: string) => void;
}) {
  const [password, setPassword] = useState("");
  const [fieldError, setFieldError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (!row) return;
    setPassword("");
    setFieldError(null);
  }, [row]);

  const submit = async () => {
    if (!row) return;
    if (password.length < MIN_PASSWORD_CHARS) {
      setFieldError(t("create.passwordTooShort"));
      return;
    }
    setFieldError(null);
    setSubmitting(true);
    try {
      await api.setPassword(row.id, password);
      onSaved();
    } catch (error) {
      onError(error instanceof ApiError ? error.message || error.code : String(error));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={row !== null} onOpenChange={(next) => (next ? undefined : onClose())}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t("password.title")}</DialogTitle>
          <DialogDescription>
            {t("password.for", { email: row?.email ?? "" })} · {t("password.hint")}
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            void submit();
          }}
          noValidate
        >
          <div>
            <label htmlFor="password-new" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("password.new")}</label>
            <Input
              id="password-new"
              className="mt-1"
              type="password"
              value={password}
              onChange={(event) => {
                setPassword(event.target.value);
                setFieldError(null);
              }}
              autoComplete="new-password"
              autoFocus
              aria-invalid={!!fieldError}
              aria-describedby={fieldError ? "password-field-error" : undefined}
            />
            {fieldError ? (
              <p id="password-field-error" className="mt-1 text-[0.72rem] text-[var(--st-danger,#dc2626)]" role="alert">{fieldError}</p>
            ) : null}
          </div>
          <DialogFooter className="mt-1">
            <Button variant="outline" size="sm" type="button" onClick={onClose} disabled={submitting}>
              {t("common.cancel")}
            </Button>
            <Button variant="success" size="sm" type="submit" disabled={busy || submitting || !password}>
              {submitting ? t("common.loading") : t("common.save")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
