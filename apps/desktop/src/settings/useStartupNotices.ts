import { isTauri } from '@tauri-apps/api/core';
import { useEffect, useRef } from 'react';
import { api } from './api';
import { useNoticeText } from './ui';

/**
 * Tells the user once, after the window opens, about a problem found at start (a library
 * that couldn't be opened, settings that had to be reset). Settings keeps showing it.
 */
export function useStartupNotices(show: (text: string) => void) {
  const noticeText = useNoticeText();
  const done = useRef(false);
  useEffect(() => {
    if (done.current || !isTauri()) return;
    done.current = true;
    api
      .startupNotices()
      .then((notices) => {
        const first = notices[0];
        if (first) show(noticeText(first));
      })
      .catch(() => {});
  }, [show, noticeText]);
}
