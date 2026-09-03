import { useNavigate } from "react-router-dom";
import { Button, Modal } from "@/components";
import { useT } from "@/lib/i18n";

export function PairScreen() {
  const navigate = useNavigate();
  const t = useT();
  const close = () => navigate("/peers");

  const steps: { label: string; body: string; cmd?: string }[] = [
    { label: t("pair.step1Label"), body: t("pair.step1"), cmd: t("pair.cmdListen") },
    { label: t("pair.step2Label"), body: t("pair.step2"), cmd: t("pair.cmdConnect") },
    { label: t("pair.step3Label"), body: t("pair.step3") },
    { label: t("pair.step4Label"), body: t("pair.step4") },
  ];

  return (
    <Modal title={t("pair.title")} onClose={close}>
      <div className="pair-body">
        <p className="muted">{t("pair.intro")}</p>
        <ol className="pair-guide">
          {steps.map((s) => (
            <li key={s.label}>
              <span className="pair-guide-label">{s.label}</span>
              <p>{s.body}</p>
              {s.cmd ? <code className="pair-cmd mono">{s.cmd}</code> : null}
            </li>
          ))}
        </ol>
        <div className="pair-actions">
          <Button variant="primary" onClick={close}>
            {t("common.done")}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
