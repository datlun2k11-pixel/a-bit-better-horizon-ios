# Mobile touch layout

In Horizon's app Settings, choose **Touch controls → Edit button layout**. This is independent of Minecraft's in-game Options screen and works before the engine is started.

The editor opens in landscape with a black canvas. Drag the movement, sneak, jump, perspective, chat, pause and inventory buttons. The canvas fills the display with the game’s pixel dimensions and GUI Scale. Drag the floating toolbar by its handle to reach any position underneath it. The toolbar's button selector brings an obscured control to the front for editing. The hotbar slots are a fixed reference belonging to the game HUD.

**Save** persists the draft on this device. **Cancel** discards it. **Reset** restores the built-in button positions in the draft; choose Save to keep the reset. Closing the editor returns the phone to portrait and the tablet to its previous orientation.

Positions use normalized centres and are clamped to the actual button size after a resize or GUI-scale change. Rendering and input read the same rectangles. When controls overlap, the last painted control owns that touch, including a control placed over the hotbar.

Controls are visible by default. **Show touch controls** changes drawing only: touch regions continue to work when hidden. It can be switched off manually before a system screenshot.

## Capture limitation

Controls currently remain visible both on-device and in system recordings. There is no automatic recording-time hiding: making them disappear from the player's screen is not a substitute for excluding them only from the video.

Apple documents a secondary-window technique for recordings made by an app's own ReplayKit recorder in [Streaming an AR experience](https://developer.apple.com/documentation/arkit/streaming-an-ar-experience). That is not a guarantee for Control Center recording, system screenshots or every OS version. Capture-only exclusion and an in-app recorder are not implemented in this change. Screenshot notifications arrive after capture; they cannot remove controls from an already-taken system screenshot.

## Implementation and compatibility

The initial implementation used public controls documentation. The subsequent source-backed comparison explicitly requested by the user inspected pinned Pojav and Amethyst GPLv3 source. The production adapter is newly written Rust, and reference method bodies remain outside this repository. See [input parity and provenance](pojav-input-parity.md) for the source pins, test boundary and repository-policy exception.

The Swift configuration uses `touchControls.v1` in this app's UserDefaults, separately from account data and world settings. Invalid data falls back to the built-in layout. Thirteen button centres cross the capacity-checked C ABI as twenty-six floats (older readers still request twenty), with NaN meaning default placement. Flag bit zero hides painting, bit one selects the virtual cursor, bit two selects joystick movement, and bits three/four disable mouse/hotbar gestures. No pointers are retained across the call. The engine resolves the bundled export once and falls back to the original controls if running with an older launcher framework. Desktop layouts are unchanged.

Validation covers production Rust layout and finger dispatch tests, isolated Swift persistence and ABI tests, iOS 18 phone and iPadOS 27 simulator editor interactions, an arm64 device framework build, and the iOS engine Clippy check. Simulator UI and compilation checks do not establish physical-device gameplay or recording behavior.

## Cursor, mouse buttons and joystick

Menu input is chosen in the game's own screen, Options → Controls → Touch Controls → **Menu Input**, as **Direct touch** (default) or **Mouse cursor**. Direct touch acts where the finger is. Mouse cursor draws a mouse arrow the game owns (iOS shows none) on every screen, options and title included, and it stays visible between touches; it is a touchpad, not a pointer that follows the hand: a drag moves the arrow by the finger's own movement and neither clicks nor scrolls by itself, a tap clicks under the arrow wherever the finger is, a two-finger drag scrolls lists as a wheel would, and a double tap that stays still for a moment holds the left button until the finger lifts, dragging whatever it grabbed; a touch that moves the arrow never presses, so moving the pointer right after a click is not a second click. Simply tapping a widget therefore clicks the arrow's spot rather than the finger's, and the ✕ and ⌨ buttons always answer the tap itself. The arrow stays where it was left between taps, is clamped to the view, and starts in the middle of a menu it opens on. A click is pressed when a stationary tap lifts and let go on a later frame, so a screen finds the widget under the pointer; screen transitions, cancellation and reloads discard stale presses.

In a world, one primary finger turns the camera; the explicit left/right buttons provide held attack/use. With gestures enabled, a short surface tap uses/right-clicks and a 400 ms long press attacks/mines until release; **Touch Interaction** on the Touch Controls page (on by default) turns those surface gestures off, so in a world the fingers only turn the camera and everything is done through the layout's buttons and their key mappings. Menus are unaffected. Two-finger menu gestures scroll or open/close the keyboard. Hotbar gestures select a slot on tap, swap the offhand on double tap, and send Drop while held. The optional joystick generates physical WASD inputs through the in-game bindings, including diagonals and a centre dead zone. Mouse buttons and joystick have independent movable positions in the full-screen editor; the joystick replaces the directional pad while keeping sneak separate. Old saved layouts retain their positions and visibility. The native bridge continues to accept older 20-float readers while new readers receive all 26 position values.

The source-backed adapter matches the tested Pojav iOS input boundary: eight direction sectors, four W/A/S/D states per sector change, a 35% travel dead zone with retained direction until release, and relative cursor scaling/clamping. In menu cursor mode, the optional joystick drives the cursor with frame-time scaled acceleration. It does not import Pojav JSON control maps or claim compatibility with every Pojav setting. Exact-axis behavior and the scope of the comparison are recorded in [input parity and provenance](pojav-input-parity.md).

Both menu modes use the same visible key/mouse buttons; direct mode positions the pointer at the finger, while cursor mode moves it by the finger's own movement. The native gesture/callback comparison, actual simulator checks and release-validation limits are documented in [input parity](pojav-input-parity.md).


## Amethyst-style controls (mobile port)

The game's own editor (Options → Controls → Touch Controls) is the layout editor. The world overlay now uses
rounded translucent labels and a circular joystick, with Amethyst default positions.
Movement remains selectable between directional buttons and joystick. The More drawer
contains perspective, chat, debug, and player list; Runtime Menu opens the pause menu.
Sprint and Shift toggle while playing; Shift remains a held modifier in menus.
Middle Click uses the pick-block binding. Hide GUI leaves its own restore button.

The editor supports resizing (pinch or Size controls), opacity, and optional forward
lock. Save applies the draft; Cancel discards it; Reset restores positions, sizes,
opacity and lock. Existing saved centres and movement selection remain readable.
The first thirteen button identifiers are unchanged. The v2 snapshot appends new
controls and carries bounded size and opacity values; older v1 bundles remain readable.

The joystick retains the current adapter's Amethyst direction and centre-retention
behavior. Forward lock starts only when the finger lifts over its target while moving
forward; touch cancellation and leaving gameplay release it. Menu cursor placement,
frozen click context, and keyboard/mouse ownership remain handled by the current adapter.
The launcher's separate touch settings screen was removed: the same layout, opacity and menu-input choices live in the in-game editor.
