# Design System

## Direction

A compact dark desktop installer inspired by familiar system setup dialogs. Dense enough to show the whole procedure without scrolling, but written for non-technical users.

## Color

Restrained strategy. All application colors use OKLCH tokens. A deep harbor blue is reserved for the primary action and active progress; neutrals carry the window.

## Typography

System sans-serif throughout. 30px title, 14px body, 12px supporting status. Monospace appears only in Diagnostics.

## Shape and elevation

Panels use 14px corners, controls use 9px corners, and status marks are circular. Borders communicate structure; shadows are limited to the outer application window.

## Motion

150–220ms state transitions only. The active status ring may rotate. Reduced-motion disables rotation and transitions.

## Layout

Single 860×600 installer window. Header, ordered task list, contextual footer, and one primary action. Diagnostics expands inline rather than opening a modal.
