import { cloneElement, useEffect, useState, type ReactElement } from 'react';
import { Tooltip, TooltipTrigger, type HoverEvent } from 'react-aria-components';

interface HoverEvents {
  onHoverStart?: (event: HoverEvent) => void;
  onHoverEnd?: (event: HoverEvent) => void;
}

interface HintProps {
  /** Text of the tooltip; usually the accessible name of the trigger. */
  label: string;
  /** Keyboard shortcut shown as a chip next to the label. */
  shortcut?: string;
  isDisabled?: boolean;
  placement?: 'end' | 'bottom';
  /** A focusable React Aria component (Button, Link). */
  children: ReactElement<HoverEvents>;
}

const DELAY_MS = 300;

/**
 * Tooltip shown on hover and keyboard focus, with an optional shortcut chip.
 * Hover is tracked here because React Aria's Link does not report it to TooltipTrigger.
 */
export function Hint({ label, shortcut, isDisabled, placement = 'end', children }: HintProps) {
  const [isOpen, setOpen] = useState(false);
  const [isHovered, setHovered] = useState(false);

  useEffect(() => {
    if (!isHovered) return;
    const timer = setTimeout(() => setOpen(true), DELAY_MS);
    return () => clearTimeout(timer);
  }, [isHovered]);

  const trigger = cloneElement(children, {
    onHoverStart: () => setHovered(true),
    onHoverEnd: () => {
      setHovered(false);
      setOpen(false);
    },
  });

  return (
    <TooltipTrigger
      isDisabled={isDisabled}
      isOpen={isOpen && !isDisabled}
      onOpenChange={setOpen}
      delay={DELAY_MS}
      closeDelay={0}
    >
      {trigger}
      <Tooltip className="hint" placement={placement} offset={10}>
        {label}
        {shortcut && <kbd className="kbd hint-kbd">{shortcut}</kbd>}
      </Tooltip>
    </TooltipTrigger>
  );
}
