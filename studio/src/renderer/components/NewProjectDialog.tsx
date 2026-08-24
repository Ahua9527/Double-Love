import { X } from "lucide-react";
import { useState } from "react";
import { projectNameError } from "../project-name";

interface NewProjectDialogProps {
  busy?: boolean;
  error?: string | null;
  onCreate: (options: { name: string }) => void;
  onClose: () => void;
  onClearError?: () => void;
}

export function NewProjectDialog({
  busy = false,
  error,
  onCreate,
  onClose,
  onClearError,
}: NewProjectDialogProps) {
  const [name, setName] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);
  const trimmedName = name.trim();

  return (
    <div
      className="studio-popover-backdrop"
      role="presentation"
      onMouseDown={() => {
        if (!busy) onClose();
      }}
    >
      <form
        className="studio-new-project-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="new-project-title"
        onMouseDown={(event) => event.stopPropagation()}
        onSubmit={(event) => {
          event.preventDefault();
          const nextError = projectNameError(name);
          setLocalError(nextError);
          if (!nextError) onCreate({ name: trimmedName });
        }}
      >
        <header>
          <div>
            <h2 id="new-project-title">新建项目</h2>
          </div>
          <button
            type="button"
            aria-label="关闭新建项目"
            disabled={busy}
            onClick={onClose}
          >
            <X size={16} />
          </button>
        </header>
        <label className="studio-project-name-field">
          <span>项目名称</span>
          <input
            autoFocus
            value={name}
            aria-invalid={Boolean(localError || error)}
            placeholder="例如：春日采访"
            onChange={(event) => {
              setName(event.target.value);
              setLocalError(null);
              onClearError?.();
            }}
          />
        </label>
        {(localError || error) && (
          <p className="studio-project-form-error" role="alert">
            {localError ?? error}
          </p>
        )}
        <footer>
          <button
            type="button"
            className="studio-secondary-button"
            disabled={busy}
            onClick={onClose}
          >
            取消
          </button>
          <button
            type="submit"
            className="studio-primary-button"
            disabled={busy}
          >
            {busy ? "正在创建…" : "创建项目"}
          </button>
        </footer>
      </form>
    </div>
  );
}
