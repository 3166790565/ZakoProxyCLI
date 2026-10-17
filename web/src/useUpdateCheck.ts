import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api } from "./api";
import type { UpdateStatus } from "./api";

const CHECK_INTERVAL = 15 * 60 * 1000;

// One authenticated owner keeps the header and the settings page in sync.
export function useUpdateCheck(authenticated: boolean) {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const session = useRef(0);
  const enabled = useRef(authenticated);
  const latestRequest = useRef<Promise<UpdateStatus> | null>(null);
  const statusRequest = useRef<Promise<UpdateStatus> | null>(null);
  const lastCheck = useRef(0);
  enabled.current = authenticated;

  useLayoutEffect(() => {
    session.current += 1;
    latestRequest.current = null;
    statusRequest.current = null;
    lastCheck.current = 0;
    if (!authenticated) setStatus(null);
  }, [authenticated]);

  const acceptStatus = useCallback((value: UpdateStatus) => {
    if (enabled.current) setStatus(value);
  }, []);

  const checkLatest = useCallback(() => {
    if (!enabled.current) return Promise.reject(new Error("请先登录"));
    if (latestRequest.current) return latestRequest.current;
    const generation = session.current;
    lastCheck.current = Date.now();
    // Finish an in-flight status poll before starting a fresh release lookup.
    const request = (statusRequest.current || Promise.resolve())
      .catch(() => undefined)
      .then(() => {
        if (!enabled.current || generation !== session.current)
          throw new Error("登录会话已变更");
        return api<UpdateStatus>("/updates/latest");
      })
      .then((value) => {
        if (generation === session.current) acceptStatus(value);
        return value;
      })
      .finally(() => {
        if (latestRequest.current === request) latestRequest.current = null;
      });
    latestRequest.current = request;
    return request;
  }, [acceptStatus]);

  const readStatus = useCallback(() => {
    if (!enabled.current) return Promise.reject(new Error("请先登录"));
    // A latest check returns status too; share it instead of racing it.
    if (latestRequest.current) return latestRequest.current;
    if (statusRequest.current) return statusRequest.current;
    const generation = session.current;
    const request = api<UpdateStatus>("/updates")
      .then((value) => {
        if (generation === session.current && !latestRequest.current)
          acceptStatus(value);
        return value;
      })
      .finally(() => {
        if (statusRequest.current === request) statusRequest.current = null;
      });
    statusRequest.current = request;
    return request;
  }, [acceptStatus]);

  useEffect(() => {
    if (!authenticated) return;
    let cancelled = false;
    let timer: number | undefined;
    async function checkIfDue() {
      window.clearTimeout(timer);
      if (cancelled || document.visibilityState === "hidden") return;
      if (Date.now() - lastCheck.current >= CHECK_INTERVAL) {
        try {
          await checkLatest();
        } catch {
          // Background checks stay quiet; manual checks show actionable errors.
        }
      }
      if (!cancelled && !document.hidden) {
        window.clearTimeout(timer);
        timer = window.setTimeout(
          () => void checkIfDue(),
          Math.max(1000, CHECK_INTERVAL - (Date.now() - lastCheck.current)),
        );
      }
    }
    const onVisibility = () => void checkIfDue();
    document.addEventListener("visibilitychange", onVisibility);
    void checkIfDue();
    return () => {
      cancelled = true;
      session.current += 1;
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [authenticated, checkLatest]);

  return { status, checkLatest, readStatus, acceptStatus };
}

export type UpdateCheck = ReturnType<typeof useUpdateCheck>;
