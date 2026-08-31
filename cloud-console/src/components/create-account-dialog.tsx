import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import * as api from "@/lib/api";
import { ApiError, type Role } from "@/lib/api";
import { MIN_PASSWORD_CHARS, t } from "@/lib/labels";
import { cn } from "@/lib/utils";

const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

export function CreateAccountDialog({
  open,
  busy,
  onClose,
  onCreated,
  onError,
}: {
  open: boolean;
  busy: boolean;
  onClose: () => void;
  onCreated: (row: api.AccountRow) => void;
  onError: (message: string) => void;
}) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [role, setRole] = useState<Role>("user");
  const [fieldError, setFieldError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (!open) return;
    setEmail("");
    setPassword("");
    setRole("user");
    setFieldError(null);
  }, [open]);

  const submit = async () => {
    const normalized = email.trim();
    if (!EMAIL_PATTERN.test(normalized)) {
      setFieldError(t("create.emailInvalid"));
      return;
    }
    if (password.length < MIN_PASSWORD_CHARS) {
      setFieldError(t("create.passwordTooShort"));
      return;
    }
    setFieldError(null);
    setSubmitting(true);
    try {
      onCreated(await api.createAccount({ email: normalized, password, role }));
    } catch (error) {
      onError(error instanceof ApiError ? error.message || error.code : String(error));
    } finally {
      setSubmitting(false);
    }
  };

  const disabled = busy || submitting || !email.trim() || !password;

  return (
    <Dialog open={open} onOpenChange={(next) => (next ? undefined : onClose())}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t("create.title")}</DialogTitle>
          <DialogDescription>{t("create.hint")}</DialogDescription>
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
            <label htmlFor="create-email" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("create.email")}</label>
            <Input
              id="create-email"
              className="mt-1"
              type="email"
              value={email}
              onChange={(event) => {
                setEmail(event.target.value);
                setFieldError(null);
              }}
              autoComplete="off"
              autoFocus
              aria-invalid={!!fieldError}
              aria-describedby={fieldError ? "create-field-error" : undefined}
            />
          </div>
          <div>
            <label htmlFor="create-password" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("create.password")}</label>
            <Input
              id="create-password"
              className="mt-1"
              type="password"
              value={password}
              onChange={(event) => {
                setPassword(event.target.value);
                setFieldError(null);
              }}
              autoComplete="new-password"
            />
          </div>
          <div>
            <span className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("create.role")}</span>
            <div className="mt-1 flex gap-1.5" role="radiogroup" aria-label={t("create.role")}>
              {(["user", "admin"] as const).map((option) => (
                <button
                  key={option}
                  type="button"
                  role="radio"
                  aria-checked={role === option}
                  onClick={() => setRole(option)}
                  className={cn(
                    "h-8 cursor-pointer rounded-[var(--r-sm,8px)] border px-3 text-[0.75rem] font-medium transition-colors duration-150",
                    role === option
                      ? "border-[var(--st-accent,#5e6ad2)] bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent,#5e6ad2)]"
                      : "border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] text-[var(--t2,#62666d)] hover:bg-[var(--surface-2,#f3f4f5)]",
                  )}
                >
                  {t(option === "admin" ? "role.admin" : "role.user")}
                </button>
              ))}
            </div>
          </div>

          {fieldError ? (
            <p id="create-field-error" className="text-[0.72rem] text-[var(--st-danger,#dc2626)]" role="alert">{fieldError}</p>
          ) : null}

          <DialogFooter className="mt-1">
            <Button variant="outline" size="sm" type="button" onClick={onClose} disabled={submitting}>
              {t("common.cancel")}
            </Button>
            <Button size="sm" type="submit" disabled={disabled}>
              {submitting ? t("create.creating") : t("create.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
