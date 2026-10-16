import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import {
  AlertTriangle,
  ArrowUpToLine,
  Check,
  CheckCircle2,
  ExternalLink,
  GitPullRequestArrow,
  LoaderCircle,
  RefreshCw,
  RotateCcw,
  Terminal,
  XCircle,
} from "lucide-react";
import { api, ApiError, errorMessage } from "./api";
import type { Release, UpdateJob, UpdateStatus } from "./api";
import { Badge, Button, ErrorBox, Field, Modal } from "./components";
import type { Notice } from "./components";

const stages = [
  { id: "fetching", label: "获取源码" },
  { id: "building", label: "构建镜像" },
  { id: "deploying", label: "部署版本" },
  { id: "verifying", label: "健康验证" },
] as const;
const terminalStatuses = new Set(["succeeded", "failed", "rolled_back"]);

function isRunning(job: UpdateJob | null | undefined) {
  return !!job && !terminalStatuses.has(job.status);
}

function jobLabel(status: string) {
  if (status === "succeeded") return "更新成功";
  if (status === "failed") return "更新失败";
  if (status === "rolled_back") return "已回退至原版本";
  return stages.find((stage) => stage.id === status)?.label || "准备更新";
}

function dateLabel(value: string | number | null | undefined) {
  if (!value) return "";
  const date = new Date(typeof value === "number" ? value * 1000 : value);
  return Number.isNaN(date.getTime())
    ? String(value)
    : date.toLocaleString("zh-CN", { hour12: false });
}

export function SystemUpdates({
  notify,
  onVersion,
}: {
  notify: Notice;
  onVersion: (version: string) => void;
}) {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [error, setError] = useState("");
  const [checking, setChecking] = useState(false);
  const [reconnecting, setReconnecting] = useState(false);
  const [confirmation, setConfirmation] = useState<Release | null>(null);
  const [password, setPassword] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState("");
  const [pollRevision, setPollRevision] = useState(0);
  const latestStatus = useRef<UpdateStatus | null>(null);
  const logs = useRef<HTMLPreElement>(null);
  const followLogs = useRef(true);
  const versionCallback = useRef(onVersion);
  versionCallback.current = onVersion;

  function acceptStatus(value: UpdateStatus) {
    if (value.error && !value.job && isRunning(latestStatus.current?.job)) {
      const preserved = {
        ...value,
        latest: value.latest || latestStatus.current!.latest,
        job: latestStatus.current!.job,
      };
      latestStatus.current = preserved;
      setStatus(preserved);
      setReconnecting(true);
      return;
    }
    latestStatus.current = value;
    setStatus(value);
    versionCallback.current(value.current_version);
    setReconnecting(false);
    setError("");
  }

  useEffect(() => {
    let cancelled = false;
    let timer: number;
    async function poll() {
      try {
        const value = await api<UpdateStatus>("/updates");
        if (cancelled) return;
        acceptStatus(value);
      } catch (failure) {
        if (
          cancelled ||
          (failure instanceof ApiError && failure.status === 401)
        )
          return;
        if (isRunning(latestStatus.current?.job)) {
          setReconnecting(true);
        } else {
          setError(errorMessage(failure));
        }
      }
      if (!cancelled)
        timer = window.setTimeout(
          () => void poll(),
          isRunning(latestStatus.current?.job) ? 2500 : 15000,
        );
    }
    void poll();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [pollRevision]);

  useEffect(() => {
    if (followLogs.current && logs.current)
      logs.current.scrollTop = logs.current.scrollHeight;
  }, [status?.job?.logs]);

  async function checkLatest() {
    setChecking(true);
    setError("");
    try {
      acceptStatus(await api<UpdateStatus>("/updates/latest"));
    } catch (failure) {
      setError(errorMessage(failure));
    } finally {
      setChecking(false);
    }
  }

  function closeConfirmation() {
    if (submitting) return;
    setConfirmation(null);
    setPassword("");
    setSubmitError("");
  }

  async function startUpdate(event: FormEvent) {
    event.preventDefault();
    if (!confirmation || !password || submitting) return;
    setSubmitting(true);
    setSubmitError("");
    try {
      const value = await api<UpdateStatus>("/updates", "POST", {
        tag: confirmation.tag,
        current_password: password,
      });
      acceptStatus(value);
      setConfirmation(null);
      setPassword("");
      followLogs.current = true;
      setPollRevision((value) => value + 1);
      notify("源码构建任务已启动");
    } catch (failure) {
      setSubmitError(errorMessage(failure));
    } finally {
      setSubmitting(false);
    }
  }

  const job = status?.job;
  const running = isRunning(job);
  const stageIndex = stages.findIndex((stage) => stage.id === job?.status);
  const latest = status?.latest;
  const rebuild = !!latest && latest.version === status?.current_version;
  const canUpdate = status?.update_available || rebuild;
  const succeeded = job?.status === "succeeded";
  const rolledBack = job?.status === "rolled_back";
  const failed = job?.status === "failed";
  const releaseLink = latest?.url?.startsWith("https://github.com/")
    ? latest.url
    : null;

  return (
    <section
      className="settings-section system-updates"
      aria-labelledby="updates-title"
    >
      <div className="settings-section-heading">
        <span className="settings-icon">
          <GitPullRequestArrow size={20} />
        </span>
        <div>
          <h2 id="updates-title">GitHub 源码更新</h2>
          <p>最新正式版</p>
        </div>
        {status && (
          <Badge variant={status.configured ? "green" : "neutral"} dot>
            {status.configured ? "已启用" : "未配置"}
          </Badge>
        )}
      </div>
      <div className="settings-section-body update-body">
        <div className="update-summary">
          <dl className="update-versions">
            <div>
              <dt>当前版本</dt>
              <dd>
                <code>{status?.current_version || "读取中…"}</code>
              </dd>
            </div>
            <div>
              <dt>最新正式版</dt>
              <dd>
                {latest ? (
                  <code>{latest.tag}</code>
                ) : (
                  <span className="muted">待检查</span>
                )}
                {rebuild && !running && <Badge variant="green">已是最新</Badge>}
                {latest && !canUpdate && !running && (
                  <Badge>当前版本较新</Badge>
                )}
                {status?.update_available && (
                  <Badge variant="amber">可更新</Badge>
                )}
              </dd>
            </div>
          </dl>
          <div className="update-actions">
            <Button
              type="button"
              onClick={() => void checkLatest()}
              busy={checking}
              disabled={running}
            >
              {!checking && <RefreshCw size={15} />}
              检查更新
            </Button>
            <Button
              type="button"
              className="primary"
              disabled={
                !status?.configured ||
                !latest ||
                !canUpdate ||
                running ||
                checking
              }
              onClick={() => {
                if (latest) setConfirmation(latest);
                setSubmitError("");
              }}
            >
              <ArrowUpToLine size={15} />
              {rebuild ? "重新构建" : "构建并更新"}
            </Button>
          </div>
        </div>
        {latest && (
          <div className="update-release-meta">
            {latest.published_at && (
              <span>发布于 {dateLabel(latest.published_at)}</span>
            )}
            {releaseLink && (
              <a href={releaseLink} target="_blank" rel="noreferrer">
                GitHub Release <ExternalLink size={13} />
              </a>
            )}
          </div>
        )}
        {latest?.notes && (
          <details className="update-release-notes">
            <summary>版本说明</summary>
            <div>{latest.notes}</div>
          </details>
        )}
        {status && !status.configured && (
          <div className="update-notice">
            <AlertTriangle size={16} />
            <span>此部署尚未启用更新服务</span>
          </div>
        )}
        <ErrorBox message={error || status?.error || ""} />
        {job && (
          <div className="update-job" aria-label="更新任务">
            <div
              className={`update-job-heading ${succeeded ? "success" : failed ? "failure" : rolledBack ? "rollback" : ""}`}
            >
              <span className="update-job-icon">
                {succeeded ? (
                  <CheckCircle2 size={19} />
                ) : failed ? (
                  <XCircle size={19} />
                ) : rolledBack ? (
                  <RotateCcw size={19} />
                ) : (
                  <LoaderCircle size={19} className="spin" />
                )}
              </span>
              <div
                className="update-job-title"
                role="status"
                aria-live="polite"
              >
                <strong>{jobLabel(job.status)}</strong>
                <span>{job.message}</span>
              </div>
              <code className="update-job-version">{job.version}</code>
            </div>
            {running && (
              <ol className="update-stages" aria-label="更新阶段">
                {stages.map((stage, index) => (
                  <li
                    key={stage.id}
                    className={
                      index === stageIndex
                        ? "current"
                        : index < stageIndex
                          ? "completed"
                          : ""
                    }
                    aria-current={index === stageIndex ? "step" : undefined}
                  >
                    <span>
                      {index < stageIndex ? <Check size={12} /> : index + 1}
                    </span>
                    {stage.label}
                  </li>
                ))}
              </ol>
            )}
            {reconnecting && (
              <div className="update-notice" role="status">
                <RefreshCw size={15} className="spin" />
                <span>服务正在重启，自动重新连接中…</span>
              </div>
            )}
            <div className="update-log-heading">
              <span>
                <Terminal size={14} /> 构建日志
              </span>
              <time>{dateLabel(job.finished_at || job.started_at)}</time>
            </div>
            <pre
              className="update-logs"
              ref={logs}
              tabIndex={0}
              aria-label="构建日志"
              onScroll={() => {
                const element = logs.current;
                if (element)
                  followLogs.current =
                    element.scrollHeight -
                      element.scrollTop -
                      element.clientHeight <
                    40;
              }}
            >
              <code>
                {job.logs.length
                  ? job.logs.slice(-400).join("\n")
                  : "等待构建日志…"}
              </code>
            </pre>
            {succeeded && (
              <div className="update-complete-actions">
                <Button type="button" onClick={() => window.location.reload()}>
                  <RefreshCw size={15} />
                  刷新页面
                </Button>
              </div>
            )}
          </div>
        )}
      </div>
      {confirmation && (
        <Modal
          title="确认源码构建更新"
          onClose={closeConfirmation}
          busy={submitting}
        >
          <form onSubmit={(event) => void startUpdate(event)}>
            <div className="modal-body update-confirmation">
              <dl className="update-confirm-version">
                <dt>目标版本</dt>
                <dd>
                  <code>{confirmation.tag}</code>
                </dd>
              </dl>
              <div className="update-notice">
                <AlertTriangle size={17} />
                <span>
                  部署时站点将短暂重启；健康验证失败后自动恢复上一版本。
                </span>
              </div>
              <Field label="管理员密码">
                <input
                  type="password"
                  autoComplete="current-password"
                  autoFocus
                  required
                  maxLength={1024}
                  value={password}
                  onChange={(event) => setPassword(event.target.value)}
                  placeholder="验证当前管理员密码"
                  disabled={submitting}
                />
              </Field>
              <ErrorBox message={submitError} />
            </div>
            <div className="modal-footer">
              <Button
                type="button"
                onClick={closeConfirmation}
                disabled={submitting}
              >
                取消
              </Button>
              <Button
                type="submit"
                className="primary"
                busy={submitting}
                disabled={!password}
              >
                {!submitting && <ArrowUpToLine size={15} />}确认构建并更新
              </Button>
            </div>
          </form>
        </Modal>
      )}
    </section>
  );
}
