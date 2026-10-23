use std::time::Duration;

use sc_keymap::usage;

use super::*;

const PC_SCREEN: Rect = Rect { left: 0, top: 0, right: 2560, bottom: 1440 };
const MAC_SCREEN: Rect = Rect { left: 0, top: 0, right: 1512, bottom: 982 };

fn config(peer_side: Side) -> Config {
    Config { peer_side, swap_ctrl_cmd: true, speed: 1.0, edge_delay: Duration::ZERO }
}

/// A Windows PC with the Mac to its right, already connected.
fn pc() -> Node {
    let mut node = Node::new(config(Side::Right), Os::Windows, "pc".into(), vec![PC_SCREEN]);
    node.peer_connected(mac().hello());
    node
}

/// A Mac with the PC to its left, already connected.
fn mac() -> Node {
    let mut node = Node::new(config(Side::Left), Os::MacOs, "mac".into(), vec![MAC_SCREEN]);
    let pc = Node::new(config(Side::Right), Os::Windows, "pc".into(), vec![PC_SCREEN]);
    node.peer_connected(pc.hello());
    node
}

fn moved(x: i32, y: i32) -> InputEvent {
    InputEvent::MouseMove { x, y }
}

fn delta(dx: i32, dy: i32) -> InputEvent {
    InputEvent::MouseDelta { dx, dy }
}

fn key(usage: u16, down: bool) -> InputEvent {
    InputEvent::Key { usage, down }
}

fn sent(actions: &[Action]) -> Vec<Message> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Send(m) => Some(m.clone()),
            _ => None,
        })
        .collect()
}

fn injected(actions: &[Action]) -> Vec<InputEvent> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Inject(e) => Some(*e),
            _ => None,
        })
        .collect()
}

#[test]
fn crossing_the_right_edge_enters_the_peer() {
    let mut pc = pc();
    assert!(sent(&pc.local_input(moved(2558, 720))).is_empty());

    let actions = pc.local_input(moved(2559, 720));
    assert!(actions.contains(&Action::Grab(true)));
    assert_eq!(sent(&actions), vec![Message::Enter { x: 0, y: 491 }]);
    assert!(pc.is_remote());
}

#[test]
fn no_crossing_without_peer_or_while_dragging() {
    let mut lonely = Node::new(config(Side::Right), Os::Windows, "pc".into(), vec![PC_SCREEN]);
    assert!(lonely.local_input(moved(2559, 720)).is_empty());

    let mut pc = pc();
    pc.local_input(InputEvent::Button { button: MouseButton::Left, down: true });
    assert!(pc.local_input(moved(2559, 720)).is_empty());
    pc.local_input(InputEvent::Button { button: MouseButton::Left, down: false });
    assert!(pc.local_input(moved(2559, 720)).contains(&Action::Grab(true)));
}

#[test]
fn wrong_edge_does_not_cross() {
    let mut pc = pc();
    assert!(pc.local_input(moved(0, 720)).is_empty());
    assert!(!pc.is_remote());
}

#[test]
fn remote_motion_is_clamped_to_peer_screens() {
    let mut pc = pc();
    pc.local_input(moved(2559, 720));

    let actions = pc.local_input(delta(100, -20));
    assert_eq!(sent(&actions), vec![Message::MouseMove { x: 100, y: 471 }]);

    let actions = pc.local_input(delta(5000, 5000));
    assert_eq!(sent(&actions), vec![Message::MouseMove { x: 1511, y: 981 }]);

    // Already in the corner: nothing new to send.
    assert!(pc.local_input(delta(10, 10)).is_empty());
}

#[test]
fn speed_scales_remote_motion() {
    let mut pc = pc();
    pc.config.speed = 0.5;
    pc.local_input(moved(2559, 720));
    let actions = pc.local_input(delta(100, 0));
    assert_eq!(sent(&actions), vec![Message::MouseMove { x: 50, y: 491 }]);
}

#[test]
fn moving_back_returns_control_at_matching_height() {
    let mut pc = pc();
    pc.local_input(moved(2559, 720));
    pc.local_input(delta(50, 0));

    let actions = pc.local_input(delta(-51, 0));
    assert_eq!(sent(&actions), vec![Message::Leave]);
    assert!(actions.contains(&Action::Grab(false)));
    assert_eq!(injected(&actions), vec![moved(2558, 720)]);
    assert!(!pc.is_remote());
}

#[test]
fn held_button_on_peer_blocks_return() {
    let mut pc = pc();
    pc.local_input(moved(2559, 720));
    pc.local_input(InputEvent::Button { button: MouseButton::Left, down: true });
    pc.local_input(delta(-10, 0));
    assert!(pc.is_remote());

    pc.local_input(InputEvent::Button { button: MouseButton::Left, down: false });
    pc.local_input(delta(-10, 0));
    assert!(!pc.is_remote());
}

fn edges(actions: &[Action]) -> Vec<Option<EdgeHint>> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Edge(h) => Some(*h),
            _ => None,
        })
        .collect()
}

fn hint(near: u8, push: u8) -> Option<EdgeHint> {
    Some(EdgeHint { side: Side::Right, x: 2559, top: 0, bottom: 1440, near, push })
}

#[test]
fn bar_shows_near_the_shared_edge_only() {
    let mut pc = pc();
    assert!(pc.local_input(moved(1000, 720)).is_empty());
    assert!(pc.local_input(moved(2559 - EDGE_NEAR, 720)).is_empty());
    assert_eq!(edges(&pc.local_input(moved(2559 - EDGE_NEAR + 1, 720))), vec![hint(1, 0)]);
    assert_eq!(edges(&pc.local_input(moved(2558, 720))), vec![hint(10, 0)]);
    // Same step: nothing new to draw.
    assert!(pc.local_input(moved(2558, 700)).is_empty());
    assert_eq!(edges(&pc.local_input(moved(1000, 720))), vec![None]);

    // Not near the other edge, and not while dragging.
    assert!(pc.local_input(moved(10, 720)).is_empty());
    pc.local_input(moved(2550, 720));
    let actions = pc.local_input(InputEvent::Button { button: MouseButton::Left, down: true });
    assert_eq!(edges(&actions), vec![None]);
    assert!(pc.local_input(moved(2558, 720)).is_empty());
}

#[test]
fn bar_hides_when_crossing() {
    let mut pc = pc();
    pc.local_input(moved(2550, 720));
    let actions = pc.local_input(moved(2559, 720));
    assert_eq!(actions[0], Action::Edge(None));
    assert_eq!(actions[1], Action::Grab(true));
}

#[test]
fn edge_delay_needs_a_push() {
    let mut node = pc();
    node.config.edge_delay = Duration::from_millis(400);
    let t0 = Instant::now();
    let at = |ms| t0 + Duration::from_millis(ms);

    assert_eq!(edges(&node.local_input_at(moved(2559, 720), at(0))), vec![hint(10, 0)]);
    assert_eq!(edges(&node.local_input_at(moved(2559, 720), at(200))), vec![hint(10, 5)]);
    assert!(!node.is_remote());
    let actions = node.local_input_at(moved(2559, 720), at(400));
    assert!(actions.contains(&Action::Grab(true)));
    assert!(node.is_remote());

    // Backing off the edge starts the push over.
    let mut node = pc();
    node.config.edge_delay = Duration::from_millis(400);
    node.local_input_at(moved(2559, 720), at(0));
    node.local_input_at(moved(2550, 720), at(300));
    node.local_input_at(moved(2559, 720), at(350));
    assert!(!node.local_input_at(moved(2559, 720), at(500)).contains(&Action::Grab(true)));
    assert!(node.local_input_at(moved(2559, 720), at(750)).contains(&Action::Grab(true)));
}

#[test]
fn multi_monitor_bar_covers_the_monitor_under_the_cursor() {
    // A short monitor to the right of the main one.
    let screens = vec![PC_SCREEN, Rect { left: 2560, top: 0, right: 4480, bottom: 1080 }];
    let mut pc = Node::new(config(Side::Right), Os::Windows, "pc".into(), screens);
    pc.peer_connected(mac().hello());
    let expected = EdgeHint { side: Side::Right, x: 4479, top: 0, bottom: 1080, near: 10, push: 0 };
    assert_eq!(edges(&pc.local_input(moved(4478, 500))), vec![Some(expected)]);
    // Next to the main monitor's right side there is no edge to the peer.
    assert_eq!(edges(&pc.local_input(moved(2558, 1200))), vec![None]);
}

#[test]
fn remote_input_is_forwarded() {
    let mut pc = pc();
    pc.local_input(moved(2559, 720));
    assert_eq!(
        sent(&pc.local_input(key(0x06, true))),
        vec![Message::Key { usage: 0x06, down: true }]
    );
    assert_eq!(
        sent(&pc.local_input(InputEvent::Wheel { dx: 0, dy: -120 })),
        vec![Message::Wheel { dx: 0, dy: -120 }]
    );
}

#[test]
fn receiver_ignores_input_until_entered() {
    let mut mac = mac();
    assert!(mac.peer_message(Message::MouseMove { x: 10, y: 10 }).is_empty());
    assert!(mac.peer_message(Message::Key { usage: 0x04, down: true }).is_empty());

    let actions = mac.peer_message(Message::Enter { x: 0, y: 491 });
    assert_eq!(injected(&actions), vec![moved(0, 491)]);
    let actions = mac.peer_message(Message::MouseMove { x: 10, y: 10 });
    assert_eq!(injected(&actions), vec![moved(10, 10)]);
}

#[test]
fn receiver_swaps_ctrl_and_command_across_os() {
    let mut mac = mac();
    mac.peer_message(Message::Enter { x: 0, y: 0 });
    let actions = mac.peer_message(Message::Key { usage: usage::LEFT_CTRL, down: true });
    assert_eq!(injected(&actions), vec![key(usage::LEFT_GUI, true)]);
    let actions = mac.peer_message(Message::Key { usage: usage::LEFT_CTRL, down: false });
    assert_eq!(injected(&actions), vec![key(usage::LEFT_GUI, false)]);

    mac.config.swap_ctrl_cmd = false;
    let actions = mac.peer_message(Message::Key { usage: usage::LEFT_CTRL, down: true });
    assert_eq!(injected(&actions), vec![key(usage::LEFT_CTRL, true)]);
}

/// What the PC injects for keys sent by the Mac.
fn pc_injects(keys: &[(u16, bool)]) -> Vec<InputEvent> {
    let mut pc = pc();
    pc.peer_message(Message::Enter { x: 0, y: 0 });
    keys.iter()
        .flat_map(|&(usage, down)| injected(&pc.peer_message(Message::Key { usage, down })))
        .collect()
}

#[test]
fn command_alone_is_a_windows_key_on_the_pc() {
    let actions = pc_injects(&[(usage::LEFT_GUI, true), (usage::LEFT_GUI, false)]);
    assert_eq!(
        actions,
        vec![
            key(usage::LEFT_CTRL, true),
            key(usage::LEFT_CTRL, false),
            key(usage::LEFT_GUI, true),
            key(usage::LEFT_GUI, false),
        ]
    );
}

#[test]
fn command_with_another_key_stays_ctrl() {
    let c = 0x06;
    let actions =
        pc_injects(&[(usage::LEFT_GUI, true), (c, true), (c, false), (usage::LEFT_GUI, false)]);
    assert_eq!(
        actions,
        vec![
            key(usage::LEFT_CTRL, true),
            key(c, true),
            key(c, false),
            key(usage::LEFT_CTRL, false)
        ]
    );

    // Cmd+click is a Ctrl+click.
    let mut pc = pc();
    pc.peer_message(Message::Enter { x: 0, y: 0 });
    pc.peer_message(Message::Key { usage: usage::LEFT_GUI, down: true });
    pc.peer_message(Message::Button { button: MouseButton::Left, down: true });
    let actions = pc.peer_message(Message::Key { usage: usage::LEFT_GUI, down: false });
    assert_eq!(injected(&actions), vec![key(usage::LEFT_CTRL, false)]);
}

#[test]
fn ctrl_alone_and_the_mac_side_are_unchanged() {
    // Ctrl from the Mac becomes the Windows key and needs no extra tap.
    let actions = pc_injects(&[(usage::LEFT_CTRL, true), (usage::LEFT_CTRL, false)]);
    assert_eq!(actions, vec![key(usage::LEFT_GUI, true), key(usage::LEFT_GUI, false)]);

    // The Windows key alone on the Mac stays a plain Ctrl press.
    let mut mac = mac();
    mac.peer_message(Message::Enter { x: 0, y: 0 });
    let mut actions = mac.peer_message(Message::Key { usage: usage::LEFT_GUI, down: true });
    actions.extend(mac.peer_message(Message::Key { usage: usage::LEFT_GUI, down: false }));
    assert_eq!(injected(&actions), vec![key(usage::LEFT_CTRL, true), key(usage::LEFT_CTRL, false)]);
}

#[test]
fn receiver_releases_held_keys_on_leave_and_disconnect() {
    for end in [Some(Message::Leave), Some(Message::ReleaseAll), None] {
        let mut mac = mac();
        mac.peer_message(Message::Enter { x: 0, y: 0 });
        mac.peer_message(Message::Key { usage: 0x04, down: true });
        mac.peer_message(Message::Button { button: MouseButton::Right, down: true });

        let actions = match end.clone() {
            Some(msg) => mac.peer_message(msg),
            None => mac.peer_disconnected(),
        };
        assert_eq!(
            injected(&actions),
            vec![key(0x04, false), InputEvent::Button { button: MouseButton::Right, down: false }],
            "{end:?}"
        );
    }
}

#[test]
fn unmatched_key_up_is_not_injected() {
    let mut mac = mac();
    mac.peer_message(Message::Enter { x: 0, y: 0 });
    assert!(mac.peer_message(Message::Key { usage: 0x04, down: false }).is_empty());
}

#[test]
fn disconnect_while_remote_ungrabs_without_sending() {
    let mut pc = pc();
    pc.local_input(moved(2559, 720));
    let actions = pc.peer_disconnected();
    assert!(actions.contains(&Action::Grab(false)));
    assert!(sent(&actions).is_empty());
    assert!(!pc.is_remote());
}

#[test]
fn emergency_release_tells_the_peer() {
    let mut pc = pc();
    pc.local_input(moved(2559, 720));
    let actions = pc.emergency_release();
    assert_eq!(sent(&actions), vec![Message::Leave]);
    assert!(!pc.is_remote());
    assert!(pc.emergency_release().is_empty());
}

#[test]
fn simultaneous_enter_yields_to_the_peer() {
    let mut pc = pc();
    pc.local_input(moved(2559, 720));
    let actions = pc.peer_message(Message::Enter { x: 2559, y: 100 });
    assert_eq!(sent(&actions), vec![Message::Leave]);
    assert!(!pc.is_remote());
    assert_eq!(injected(&actions), vec![moved(2559, 100)]);
}

/// Wires two nodes together and walks the cursor PC -> Mac -> PC.
#[test]
fn round_trip_between_two_nodes() {
    let mut pc = pc();
    let mut mac = mac();
    let mut mac_injected = Vec::new();
    let mut deliver = |actions: Vec<Action>, mac: &mut Node| {
        for msg in sent(&actions) {
            mac_injected.extend(injected(&mac.peer_message(msg)));
        }
    };

    deliver(pc.local_input(moved(2559, 1439)), &mut mac);
    deliver(pc.local_input(delta(300, -100)), &mut mac);
    deliver(pc.local_input(key(usage::LEFT_CTRL, true)), &mut mac);
    deliver(pc.local_input(key(0x06, true)), &mut mac); // C
    deliver(pc.local_input(delta(-400, 0)), &mut mac);

    assert!(!pc.is_remote());
    assert_eq!(
        mac_injected,
        vec![
            moved(0, 981),
            moved(300, 881),
            key(usage::LEFT_GUI, true), // Ctrl+C arrives as Cmd+C
            key(0x06, true),
            // Released on Leave; modifiers (0xE0..) sort last.
            key(0x06, false),
            key(usage::LEFT_GUI, false),
        ]
    );
}

#[test]
fn clipboard_goes_along_with_control() {
    // Crossing over shares this machine's clipboard, before the peer takes over.
    let mut pc = pc();
    let actions = pc.local_input(moved(2559, 720));
    let share = actions.iter().position(|a| *a == Action::ShareClipboard).unwrap();
    let enter = actions.iter().position(|a| matches!(a, Action::Send(Message::Enter { .. })));
    assert!(share < enter.unwrap());

    // Getting control back makes the peer share its clipboard.
    let mut mac = mac();
    mac.peer_message(Message::Enter { x: 0, y: 0 });
    assert!(mac.peer_message(Message::Leave).contains(&Action::ShareClipboard));
    // A Leave without having been controlled shares nothing.
    assert!(!mac.peer_message(Message::Leave).contains(&Action::ShareClipboard));
}

#[test]
fn received_clipboard_is_set_at_any_time() {
    let mut mac = mac();
    let text = "kopyalanan metin".to_string();
    let actions = mac.peer_message(Message::Clipboard { text: text.clone() });
    assert_eq!(actions, vec![Action::SetClipboard(text)]);
}
