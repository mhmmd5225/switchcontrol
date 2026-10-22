use sc_keymap::usage;

use super::*;

const PC_SCREEN: Rect = Rect { left: 0, top: 0, right: 2560, bottom: 1440 };
const MAC_SCREEN: Rect = Rect { left: 0, top: 0, right: 1512, bottom: 982 };

fn config(peer_side: Side) -> Config {
    Config { peer_side, swap_ctrl_cmd: true, speed: 1.0 }
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
    assert!(pc.local_input(moved(2558, 720)).is_empty());

    let actions = pc.local_input(moved(2559, 720));
    assert_eq!(actions[0], Action::Grab(true));
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
    assert!(!pc.local_input(moved(2559, 720)).is_empty());
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

#[test]
fn keys_held_while_crossing_are_released_locally() {
    let mut pc = pc();
    pc.local_input(key(usage::LEFT_SHIFT, true));
    let actions = pc.local_input(moved(2559, 720));
    assert_eq!(injected(&actions), vec![key(usage::LEFT_SHIFT, false)]);
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
