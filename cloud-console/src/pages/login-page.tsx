import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Cloud, Eye, EyeOff, ShieldAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { useAuth } from "@/providers/auth";
import * as api from "@/lib/api";
import { t } from "@/lib/labels";
import { ApiError } from "@/lib/api";

const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

/** Turn the console error codes into operator-facing text; anything else shows the server message. */
function describeError(error: unknown): string {
  if (error instanceof ApiError) {
    switch (error.code) {
      case "ADMIN_NOT_CONFIGURED":
        return t("login.notConfigured");
      case "ADMIN_FORBIDDEN":
        return t("login.notAdmin");
      case "CLOUD_AUTH_FAILED":
        return t("login.authFailed");
      default:
        return error.message || t("common.unknownError");
    }
  }
  return error instanceof Error ? error.message : String(error);
}

export function LoginPage() {
  const { signIn } = useAuth();
  const navigate = useNavigate();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [fieldError, setFieldError] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [status, setStatus] = useState<api.AdminStatus | null>(null);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    let active = true;
    api
      .status()
      .then((next) => active && setStatus(next))
      .catch(() => active && setStatus({ admin_available: true, console_ready: false }));
    return () => {
      active = false;
    };
  }, []);

  const submit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const normalized = email.trim();
    if (!normalized || !EMAIL_PATTERN.test(normalized)) {
      setFieldError(normalized ? t("login.emailInvalid") : t("login.emailRequired"));
      return;
    }
    if (!password) {
      setFieldError(t("login.passwordRequired"));
      return;
    }
    setFieldError(null);
    setFormError(null);
    setSubmitting(true);
    try {
      await signIn(normalized, password);
      // The credential never outlives this handler.
      setPassword("");
      navigate("/accounts", { replace: true });
    } catch (error) {
      setFormError(describeError(error));
    } finally {
      setSubmitting(false);
    }
  };

  const notConfigured = status ? !status.admin_available : false;

  return (
    <div className="grid min-h-full place-items-center p-6">
      <div className="w-full max-w-sm">
        <div className="mb-4 flex items-center gap-2">
          <span className="grid size-7 place-items-center rounded-[var(--r-sm,8px)] bg-[var(--st-accent,#5e6ad2)] text-white">
            <Cloud className="size-4" />
          </span>
          <span className="text-[0.92rem] font-semibold text-[var(--t1,#222326)]">{t("app.title")}</span>
        </div>

        <Card className="p-4">
          <h1 className="text-[0.95rem] font-semibold text-[var(--t1,#222326)]">{t("login.title")}</h1>
          <p className="mt-1 text-[0.75rem] leading-relaxed text-[var(--t3,#8a8f98)]">{t("login.subtitle")}</p>

          {notConfigured ? (
            <div className="mt-3 flex gap-2 rounded-[var(--r-sm,8px)] border border-[var(--st-warn-line,#f0dcb0)] bg-[var(--st-warn-tint,#fff8e1)] p-3">
              <ShieldAlert className="mt-0.5 size-4 shrink-0 text-[var(--st-warn,#9a6700)]" />
              <div>
                <p className="text-[0.78rem] font-semibold text-[var(--st-warn,#9a6700)]">{t("login.notConfiguredTitle")}</p>
                <p className="mt-1 text-[0.72rem] leading-relaxed text-[var(--st-warn,#9a6700)]">{t("login.notConfigured")}</p>
              </div>
            </div>
          ) : (
            <form className="mt-3 flex flex-col gap-3" onSubmit={(event) => void submit(event)} noValidate>
              <div>
                <label htmlFor="admin-email" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("login.email")}</label>
                <Input
                  id="admin-email"
                  className="mt-1"
                  type="email"
                  value={email}
                  onChange={(event) => {
                    setEmail(event.target.value);
                    setFieldError(null);
                  }}
                  autoComplete="username"
                  autoFocus
                  required
                  aria-required="true"
                  aria-invalid={!!fieldError}
                  aria-describedby={fieldError ? "login-field-error" : undefined}
                  disabled={submitting}
                />
              </div>
              <div>
                <label htmlFor="admin-password" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("login.password")}</label>
                <div className="relative mt-1">
                  <Input
                    id="admin-password"
                    className="pr-10"
                    type={showPassword ? "text" : "password"}
                    value={password}
                    onChange={(event) => {
                      setPassword(event.target.value);
                      setFieldError(null);
                      setFormError(null);
                    }}
                    autoComplete="current-password"
                    required
                    aria-required="true"
                    disabled={submitting}
                  />
                  <button
                    type="button"
                    className="absolute right-1 top-1/2 inline-flex size-6 -translate-y-1/2 cursor-pointer items-center justify-center rounded-[var(--r-sm,8px)] text-[var(--t3,#8a8f98)] transition-colors duration-150 hover:bg-[var(--surface-2,#f3f4f5)] hover:text-[var(--t1,#222326)] focus-visible:outline-2 focus-visible:outline-[var(--st-accent,#5e6ad2)]"
                    onClick={() => setShowPassword((visible) => !visible)}
                    aria-label={showPassword ? t("login.hidePassword") : t("login.showPassword")}
                  >
                    {showPassword ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
                  </button>
                </div>
              </div>

              {fieldError ? (
                <p id="login-field-error" className="text-[0.72rem] text-[var(--st-danger,#dc2626)]" role="alert">{fieldError}</p>
              ) : null}
              {formError ? (
                <p className="rounded-[var(--r-sm,8px)] border border-red-200 bg-[var(--st-danger-tint,#fdecec)] px-3 py-2 text-[0.75rem] text-[var(--st-danger,#dc2626)]" role="alert">
                  {formError}
                </p>
              ) : null}

              <Button type="submit" size="sm" disabled={submitting || !email.trim() || !password}>
                {submitting ? t("login.signingIn") : t("login.submit")}
              </Button>
            </form>
          )}

          <p className="mt-3 text-[0.72rem] text-[var(--t3,#8a8f98)]">
            {t("login.endpointHint", { endpoint: window.location.origin })}
          </p>
        </Card>
      </div>
    </div>
  );
}
