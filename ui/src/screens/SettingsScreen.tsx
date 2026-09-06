import { useState } from "react";
import { Power, RotateCw } from "lucide-react";
import { useDaemonStatus, useFerry, useIdentity } from "@/lib/query";
import { Button, ConfirmDialog, CopyButton, Fingerprint } from "@/components";
import { LOCALES, LOCALE_LABELS, useI18n } from "@/lib/i18n";
import { THEME_PREFS, useTheme } from "@/lib/theme";
import { Async, ScreenHeader } from "./parts";

export function SettingsScreen() {
  const identity = useIdentity();
  const status = useDaemonStatus();
  const { client } = useFerry();
  const { locale, setLocale, t } = useI18n();
  const { pref: themePref, setPref: setThemePref } = useTheme();
  const [confirmQuit, setConfirmQuit] = useState(false);

  const canManageLifecycle = client.host !== "browser";

  return (
    <div className="screen">
      <ScreenHeader title={t("settings.title")} subtitle={t("settings.subtitle")} />

      <Async query={identity}>
        {(id) => (
          <>
            <section className="detail-section">
              <h2>{t("settings.thisDevice")}</h2>
              <div className="settings-row">
                <span className="detail-label">{t("settings.identityFingerprint")}</span>
                <span className="settings-value">
                  <Fingerprint value={id.fingerprint} />
                  <CopyButton text={id.fingerprint} />
                </span>
              </div>
              <div className="settings-row">
                <span className="detail-label">{t("settings.displayName")}</span>
                <span className="settings-value">{id.display_name}</span>
              </div>
              <div className="settings-row">
                <span className="detail-label">{t("settings.autoAccept")}</span>
                <span className="settings-value">
                  {id.auto_accept_from_roster ? t("common.on") : t("common.off")}
                  <span className="muted">{t("settings.autoAcceptHint")}</span>
                </span>
              </div>
              <div className="settings-row">
                <span className="detail-label">{t("settings.language")}</span>
                <span className="settings-value">
                  <select
                    className="text-input"
                    value={locale}
                    onChange={(e) => setLocale(e.target.value as (typeof LOCALES)[number])}
                  >
                    {LOCALES.map((l) => (
                      <option key={l} value={l}>
                        {LOCALE_LABELS[l]}
                      </option>
                    ))}
                  </select>
                  <span className="muted"> {t("settings.languageHint")}</span>
                </span>
              </div>
              <div className="settings-row">
                <span className="detail-label">{t("settings.theme")}</span>
                <span className="settings-value">
                  <select
                    className="text-input"
                    value={themePref}
                    onChange={(e) => setThemePref(e.target.value as (typeof THEME_PREFS)[number])}
                  >
                    {THEME_PREFS.map((p) => (
                      <option key={p} value={p}>
                        {t(p === "light" ? "settings.themeLight" : p === "dark" ? "settings.themeDark" : "settings.themeSystem")}
                      </option>
                    ))}
                  </select>
                  <span className="muted"> {t("settings.themeHint")}</span>
                </span>
              </div>
            </section>

            <section className="detail-section">
              <h2>{t("settings.processControl")}</h2>
              <p className="muted">{t("settings.processControlHint")}</p>
              <div className="row-actions">
                <Button
                  disabled={!canManageLifecycle}
                  title={canManageLifecycle ? t("settings.restartHint") : t("settings.restartHostManaged")}
                >
                  <RotateCw size={13} /> {t("settings.restart")}
                </Button>
                <Button variant="danger" onClick={() => setConfirmQuit(true)}>
                  <Power size={13} /> {t("settings.quit")}
                </Button>
              </div>
            </section>

            <section className="detail-section">
              <h2>{t("settings.daemonRuntime")}</h2>
              <div className="settings-grid">
                <div>
                  <span className="detail-label">{t("settings.state")}</span>
                  <span className="detail-value">
                    {status.data?.store_ok ? t("settings.running") : t("settings.degraded")}
                  </span>
                </div>
                <div>
                  <span className="detail-label">{t("settings.listenPort")}</span>
                  <span className="detail-value mono">{id.listen_port}</span>
                </div>
                <div>
                  <span className="detail-label">{t("settings.protocol")}</span>
                  <span className="detail-value mono">v{id.protocol_version}</span>
                </div>
                <div>
                  <span className="detail-label">{t("settings.dataDirectory")}</span>
                  <span className="detail-value mono ellipsis">{id.data_dir}</span>
                </div>
                <div>
                  <span className="detail-label">{t("settings.discovery")}</span>
                  <span className="detail-value">
                    {status.data?.discovery_ok ? t("settings.ok") : t("settings.unavailable")}
                  </span>
                </div>
                <div>
                  <span className="detail-label">{t("settings.transport")}</span>
                  <span className="detail-value">
                    {status.data?.transport_ok ? t("settings.ok") : t("settings.unavailable")}
                  </span>
                </div>
              </div>
            </section>
          </>
        )}
      </Async>

      {confirmQuit ? (
        <ConfirmDialog
          title={t("settings.quitTitle")}
          danger
          typeToConfirm="quit"
          confirmLabel={t("settings.quitConfirm")}
          body={<p>{t("settings.quitBody")}</p>}
          onCancel={() => setConfirmQuit(false)}
          onConfirm={() => {
            void client.quit();
            setConfirmQuit(false);
          }}
        />
      ) : null}
    </div>
  );
}
