import { useEffect, useRef } from "react";

const START_EVENT = "cc-switch:onboarding-tour-start";

/** 从任何地方（如关于页）重新打开使用教程；由 App 里常驻的 FirstRunNoticeDialog 接住 */
export function startOnboardingTour() {
  window.dispatchEvent(new Event(START_EVENT));
}

export function useOnboardingTourRequest(onRequest: () => void) {
  const ref = useRef(onRequest);
  ref.current = onRequest;
  useEffect(() => {
    const handler = () => ref.current();
    window.addEventListener(START_EVENT, handler);
    return () => window.removeEventListener(START_EVENT, handler);
  }, []);
}
