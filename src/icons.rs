//! Icons: SVG stand-ins for the SF Symbols ClashBar uses (drawn black,
//! tinted where shown), and MihoBar's own brand marks for the menu bar.

fn svg_doc(body: &str) -> String {
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24' viewBox='0 0 24 24' fill='none' stroke='#000' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'>{body}</svg>"
    )
}

fn svg(body: &str) -> String {
    format!("data:image/svg+xml;utf8,{}", percent(&svg_doc(body)))
}

fn percent(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for ch in s.chars() {
        match ch {
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '#' => out.push_str("%23"),
            '"' => out.push_str("%22"),
            '%' => out.push_str("%25"),
            ' ' => out.push_str("%20"),
            _ => out.push(ch),
        }
    }
    out
}

macro_rules! icons {
    ($($name:ident => $body:expr;)*) => {
        $(pub fn $name() -> String { svg($body) })*
    };
}

icons! {
    // The advanced window (zashboard's navigation and usage icons).
    z_connections => "<path d='M7 7h13'/><path d='M16 3l4 4-4 4'/><path d='M17 17H4'/><path d='M8 13l-4 4 4 4'/>";
    z_usage => "<path d='M4 20V10'/><path d='M10 20V4'/><path d='M16 20v-7'/><path d='M22 20H2'/>";
    z_logs => "<path d='M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z'/><path d='M14 3v5h5'/><path d='M9 13h6'/><path d='M9 17h6'/>";
    z_rules => "<rect x='3' y='3' width='7' height='18' rx='2'/><path d='M10 8.5l4.6-3.4a2 2 0 0 1 2.8.4l1.2 1.6a2 2 0 0 1-.4 2.8L10 15'/><path d='M10 18h9a2 2 0 0 0 2-2v-2a2 2 0 0 0-2-2h-3'/>";
    z_devices => "<rect x='3' y='3' width='7' height='7' rx='1.5'/><rect x='14' y='3' width='7' height='7' rx='1.5'/><rect x='3' y='14' width='7' height='7' rx='1.5'/><rect x='14' y='14' width='7' height='7' rx='1.5'/>";
    z_user => "<circle cx='12' cy='8' r='4'/><path d='M4 21c0-4 3.6-7 8-7s8 3 8 7'/>";
    z_globe => "<circle cx='12' cy='12' r='9'/><path d='M3 12h18'/><path d='M12 3c2.5 2.7 3.8 5.7 3.8 9s-1.3 6.3-3.8 9c-2.5-2.7-3.8-5.7-3.8-9S9.5 5.7 12 3z'/>";
    z_cpu => "<rect x='6' y='6' width='12' height='12' rx='2'/><rect x='9' y='9' width='6' height='6'/><path d='M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4'/>";
    z_hashtag => "<path d='M5 9h15'/><path d='M4 15h15'/><path d='M10 3L8 21'/><path d='M16 3l-2 18'/>";
    z_cloud_up => "<path d='M7 18a5 5 0 0 1-.9-9.9A6 6 0 0 1 17.7 7 4.5 4.5 0 0 1 17 18'/><path d='M12 12v8'/><path d='M9 15l3-3 3 3'/>";
    z_cloud_down => "<path d='M7 18a5 5 0 0 1-.9-9.9A6 6 0 0 1 17.7 7 4.5 4.5 0 0 1 17 18'/><path d='M12 12v8'/><path d='M9 17l3 3 3-3'/>";
    z_server => "<rect x='3' y='4' width='18' height='7' rx='2'/><rect x='3' y='13' width='18' height='7' rx='2'/><path d='M7 7.5h.01M7 16.5h.01'/>";
    z_bars => "<path d='M4 6h16'/><path d='M4 12h16'/><path d='M4 18h10'/>";
    z_arrow_down => "<path d='M12 4v16'/><path d='M6 14l6 6 6-6'/>";
    z_arrow_up => "<path d='M12 20V4'/><path d='M6 10l6-6 6 6'/>";
    z_memory => "<rect x='3' y='7' width='18' height='10' rx='2'/><path d='M7 7v10M11 7v10M15 7v10'/><path d='M7 17v3M17 17v3'/>";
    z_chevron_down => "<path d='M6 9l6 6 6-6'/>";
    z_funnel => "<path d='M3 4h18l-7 8v6l-4 2v-8z'/>";
    z_tools => "<path d='M14.7 6.3a4 4 0 0 0-5.4 5.2L3 17.8V21h3.2l6.3-6.3a4 4 0 0 0 5.2-5.4l-2.6 2.6-2.4-.6-.6-2.4z'/><path d='M16 16l4 4'/><path d='M18 14l-4 4'/>";
    z_link => "<path d='M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1'/><path d='M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1'/>";
    z_link_off => "<path d='M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1'/><path d='M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1'/><path d='M3 3l18 18'/>";
    z_pause => "<path d='M9 5v14'/><path d='M15 5v14'/>";
    z_play => "<path d='M7 4l13 8-13 8z'/>";
    z_xmark => "<path d='M6 6l12 12'/><path d='M18 6L6 18'/>";
    z_zoom_in => "<circle cx='10.5' cy='10.5' r='6.5'/><path d='M15.5 15.5L21 21'/><path d='M10.5 8v5M8 10.5h5'/>";
    z_zoom_out => "<circle cx='10.5' cy='10.5' r='6.5'/><path d='M15.5 15.5L21 21'/><path d='M8 10.5h5'/>";
    z_sort_up => "<circle cx='12' cy='12' r='9'/><path d='M12 16V8'/><path d='M8.5 11.5L12 8l3.5 3.5'/>";
    z_sort_down => "<circle cx='12' cy='12' r='9'/><path d='M12 8v8'/><path d='M8.5 12.5L12 16l3.5-3.5'/>";
    z_chain_arrow => "<circle cx='12' cy='12' r='9'/><path d='M8 12h8'/><path d='M13 9l3 3-3 3'/>";
    z_up => "<path d='M12 19V5'/><path d='M6 11l6-6 6 6'/>";
    z_down => "<path d='M12 5v14'/><path d='M6 13l6 6 6-6'/>";
    z_copy => "<rect x='8' y='8' width='12' height='12' rx='2'/><path d='M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2'/>";
    z_check => "<path d='M5 12l5 5 9-10'/>";
    z_download => "<path d='M12 4v11'/><path d='M7 10l5 5 5-5'/><path d='M4 17v2a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-2'/>";
    z_refresh => "<path d='M20 11a8 8 0 0 0-14.6-4.5L4 8'/><path d='M4 4v4h4'/><path d='M4 13a8 8 0 0 0 14.6 4.5L20 16'/><path d='M20 20v-4h-4'/>";
    z_info => "<circle cx='12' cy='12' r='9'/><path d='M12 11v5'/><path d='M12 8h.01'/>";
    z_chevron_small => "<path d='M10 7l5 5-5 5'/>";
    z_chevron_right => "<path d='M9 6l6 6-6 6'/>";
    // Header actions.
    pin => "<path d='M12 17v5'/><path d='M9 10.76V6h6v4.76l2 3.24H7z'/><path d='M8 2h8'/>";
    pin_fill => "<path d='M12 17v5'/><path d='M9 10.76V6h6v4.76l2 3.24H7z' fill='#000'/><path d='M8 2h8'/>";
    arrow_clockwise => "<path d='M21 12a9 9 0 1 1-2.64-6.36'/><path d='M21 3v6h-6'/>";
    play_circle => "<circle cx='12' cy='12' r='10'/><path d='M10 8l6 4-6 4z' fill='#000'/>";
    stop_circle => "<circle cx='12' cy='12' r='10'/><rect x='9' y='9' width='6' height='6' rx='1' fill='#000'/>";
    power => "<path d='M12 2v10'/><path d='M18.4 6.6a9 9 0 1 1-12.77.04'/>";
    safari => "<circle cx='12' cy='12' r='10'/><path d='M16.24 7.76l-2.12 6.36-6.36 2.12 2.12-6.36z'/>";
    chevron_up_down => "<path d='M7 9l5-5 5 5'/><path d='M7 15l5 5 5-5'/>";
    warning_triangle => "<path d='M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z' fill='#000'/><path d='M12 9v4' stroke='#fff'/><path d='M12 17h.01' stroke='#fff'/>";
    cpu => "<rect x='4' y='4' width='16' height='16' rx='2'/><rect x='9' y='9' width='6' height='6'/><path d='M9 1v3M15 1v3M9 20v3M15 20v3M20 9h3M20 14h3M1 9h3M1 14h3'/>";
    // Modes.
    shield => "<path d='M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z'/><path d='M12 2v20' /><path d='M12 5l-6 2.2V12c0 4.2 4.6 7.2 6 8z' fill='#000' stroke='none'/>";
    globe => "<circle cx='12' cy='12' r='10'/><path d='M2 12h20'/><path d='M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z'/>";
    arrow_direct => "<path d='M5 12h14'/><path d='M13 6l6 6-6 6'/>";
    // Config actions.
    folder => "<path d='M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z'/>";
    import_file => "<path d='M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z'/><path d='M14 2v6h6'/><path d='M12 18v-6'/><path d='M9 15l3 3 3-3'/>";
    checkmark => "<path d='M20 6L9 17l-5-5'/>";
    // Proxy tab.
    link => "<path d='M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71'/><path d='M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71'/>";
    arrow_up_circle => "<circle cx='12' cy='12' r='10'/><path d='M12 16V8M8 12l4-4 4 4'/>";
    arrow_down_circle => "<circle cx='12' cy='12' r='10'/><path d='M12 8v8M8 12l4 4 4-4'/>";
    memorychip => "<rect x='5' y='3' width='14' height='18' rx='2'/><path d='M9 7h6v6H9z'/><path d='M2 8h3M2 12h3M2 16h3M19 8h3M19 12h3M19 16h3'/>";
    chevron_right => "<path d='M9 6l6 6-6 6'/>";
    chevron_down => "<path d='M6 9l6 6 6-6'/>";
    terminal => "<rect x='2' y='4' width='20' height='16' rx='2'/><path d='M6 9l3 3-3 3M12 15h6'/>";
    doc_on_doc => "<rect x='9' y='9' width='12' height='12' rx='2'/><path d='M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1'/>";
    checkmark_circle_fill => "<circle cx='12' cy='12' r='10' fill='#000'/><path d='M8 12l3 3 5-6' stroke='#fff'/>";
    circle => "<circle cx='12' cy='12' r='9'/>";
    externaldrive => "<rect x='2' y='13' width='20' height='8' rx='2' fill='#000'/><path d='M5 13l2.5-8h9L19 13'/><circle cx='17' cy='17' r='1' fill='#fff' stroke='none'/>";
    arrows_circlepath => "<path d='M21 12a9 9 0 0 1-15.36 6.36L3 16'/><path d='M3 21v-5h5'/><path d='M3 12a9 9 0 0 1 15.36-6.36L21 8'/><path d='M21 3v5h-5'/>";
    gauge => "<path d='M12 14l4-4'/><path d='M3.34 19a10 10 0 1 1 17.32 0'/>";
    timer => "<circle cx='12' cy='13' r='8'/><path d='M12 9v4l2 2M9 2h6'/>";
    list_number => "<path d='M10 6h11M10 12h11M10 18h11M4 6h1v4M4 10h2M6 18H4c0-1 2-2 2-3s-1-1.5-2-1'/>";
    chart_bar => "<path d='M3 3v18h18'/><path d='M8 17v-4M13 17V9M18 17v-7'/>";
    chart_bar_xaxis => "<path d='M3 21h18'/><path d='M6 17v-5M10 17V8M14 17v-7M18 17V5'/>";
    eye => "<path d='M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z'/><circle cx='12' cy='12' r='3'/>";
    eye_slash => "<path d='M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19'/><path d='M1 1l22 22'/>";
    nodes_graph => "<circle cx='5' cy='12' r='2'/><circle cx='19' cy='5' r='2'/><circle cx='19' cy='19' r='2'/><path d='M7 11l10-5M7 13l10 5' stroke-dasharray='2 2'/>";
    // Connections, logs, rules.
    xmark => "<path d='M6 6l12 12M18 6L6 18'/>";
    filter_lines => "<circle cx='12' cy='12' r='10'/><path d='M7 9h10M9 12.5h6M11 16h2'/>";
    arrow_up_arrow_down => "<path d='M7 20V4M3 8l4-4 4 4'/><path d='M17 4v16M13 16l4 4 4-4'/>";
    clock => "<circle cx='12' cy='12' r='9'/><path d='M12 7v5l3 2'/>";
    network => "<circle cx='12' cy='5' r='2.5'/><circle cx='5' cy='19' r='2.5'/><circle cx='19' cy='19' r='2.5'/><path d='M12 7.5V12M12 12l-5.5 5M12 12l5.5 5'/>";
    arrow_up => "<path d='M12 20V4M5 11l7-7 7 7'/>";
    arrow_down => "<path d='M12 4v16M5 13l7 7 7-7'/>";
    info_circle_fill => "<circle cx='12' cy='12' r='10' fill='#000'/><path d='M12 11v6M12 7.5h.01' stroke='#fff'/>";
    octagon_fill => "<path d='M7.86 2h8.28L22 7.86v8.28L16.14 22H7.86L2 16.14V7.86z' fill='#000'/><path d='M12 7v6M12 16.5h.01' stroke='#fff'/>";
    text_alignleft => "<path d='M3 6h18M3 10h12M3 14h18M3 18h12'/>";
    trash => "<path d='M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14'/>";
    magnifier => "<circle cx='11' cy='11' r='7'/><path d='M21 21l-5-5'/>";
    icloud => "<path d='M7 18a5 5 0 0 1-.6-9.96A6 6 0 0 1 18 9.5a4.25 4.25 0 0 1-.5 8.5z' fill='#000'/>";
    lock => "<rect x='5' y='11' width='14' height='10' rx='2' fill='#000'/><path d='M8 11V7a4 4 0 0 1 8 0v4'/>";
    cart => "<path d='M3 3h2l2.4 12h11l2-8H6.2' /><circle cx='9' cy='20' r='1.5' fill='#000'/><circle cx='18' cy='20' r='1.5' fill='#000'/>";
    radiowaves => "<circle cx='12' cy='12' r='2' fill='#000'/><path d='M8.5 8.5a5 5 0 0 0 0 7M15.5 8.5a5 5 0 0 1 0 7M5.6 5.6a9 9 0 0 0 0 12.8M18.4 5.6a9 9 0 0 1 0 12.8'/>";
    list_rect => "<rect x='3' y='4' width='18' height='16' rx='2' fill='#000'/><path d='M7 9h10M7 12h10M7 15h6' stroke='#fff'/>";
    grid_fill => "<circle cx='7' cy='7' r='3' fill='#000'/><circle cx='17' cy='7' r='3' fill='#000'/><circle cx='7' cy='17' r='3' fill='#000'/><circle cx='17' cy='17' r='3' fill='#000'/>";
    globe_fill => "<circle cx='12' cy='12' r='10' fill='#000'/><path d='M2 12h20M12 2c3 3 3 17 0 20M12 2c-3 3-3 17 0 20' stroke='#fff' stroke-width='1.5'/>";
    group_rects => "<rect x='3' y='3' width='18' height='5' rx='1.5'/><rect x='3' y='10' width='18' height='5' rx='1.5'/><rect x='3' y='17' width='18' height='4' rx='1.5'/>";
    // Settings.
    sliders => "<path d='M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0'/><circle cx='16' cy='6' r='2'/><circle cx='10' cy='12' r='2'/><circle cx='18' cy='18' r='2'/>";
    person_check => "<circle cx='10' cy='8' r='4'/><path d='M2 21a8 8 0 0 1 13-6'/><path d='M16 19l2 2 4-4'/>";
    power_circle => "<circle cx='12' cy='12' r='10'/><path d='M12 7v5'/><path d='M8.5 9a5 5 0 1 0 7 0'/>";
    menubar => "<rect x='2' y='4' width='20' height='16' rx='2'/><path d='M2 8h20'/>";
    book => "<path d='M4 19.5A2.5 2.5 0 0 1 6.5 17H20V3H6.5A2.5 2.5 0 0 0 4 5.5z'/><path d='M8 7h8M8 11h6'/>";
    half_circle => "<circle cx='12' cy='12' r='10'/><path d='M12 2a10 10 0 0 0 0 20z' fill='#000'/>";
    gears => "<circle cx='9' cy='9' r='3'/><path d='M9 2v2M9 14v2M2 9h2M14 9h2M4 4l1.5 1.5M12.5 12.5L14 14M4 14l1.5-1.5M12.5 5.5L14 4'/><circle cx='17' cy='17' r='2.5'/><path d='M17 12.5v1.5M17 20v1.5M12.5 17H14M20 17h1.5'/>";
    wave => "<path d='M2 12c2-4 4-4 6 0s4 4 6 0 4-4 6 0'/>";
    merge => "<path d='M6 3v6a6 6 0 0 0 6 6h6'/><path d='M6 21v-6M15 12l3 3-3 3'/>";
    turn_right => "<path d='M4 20v-7a4 4 0 0 1 4-4h12'/><path d='M16 5l4 4-4 4'/>";
    wrench => "<path d='M14.7 6.3a4 4 0 0 0-5.4 5.4L3 18l3 3 6.3-6.3a4 4 0 0 0 5.4-5.4l-2.5 2.5-2.4-.6-.6-2.4z'/>";
    drive_minus => "<rect x='2' y='13' width='20' height='8' rx='2'/><path d='M5 13l2.5-8h9L19 13M9 17h6'/>";
    shield_network => "<path d='M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z'/><path d='M9 12h6M12 9v6'/>";
    arrow_down_circle_line => "<circle cx='12' cy='12' r='10'/><path d='M12 7v9M8 12l4 4 4-4'/>";
    desktop => "<rect x='2' y='3' width='20' height='14' rx='2'/><path d='M8 21h8M12 17v4'/>";
    plus => "<path d='M12 5v14M5 12h14'/>";
    pencil => "<path d='M17 3l4 4L8 20H4v-4z'/><path d='M14 6l4 4'/>";
    chevron_left => "<path d='M15 6l-6 6 6 6'/>";
    gear => "<circle cx='12' cy='12' r='3'/><path d='M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z'/>";
    shuffle => "<path d='M16 3h5v5M4 20L21 3M21 16v5h-5M15 15l6 6M4 4l5 5'/>";
    pulse => "<path d='M22 12h-4l-3 9L9 3l-3 9H2'/>";
    chevron_up_down_small => "<path d='M8 10l4-4 4 4'/><path d='M8 14l4 4 4-4'/>";
    doc_text => "<path d='M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z'/><path d='M14 2v6h6'/><path d='M8 13h8M8 17h5'/>";
}

/// MihoBar's cat (an original drawing): its head with pointed ears.
const CAT_HEAD: &str = "M4.2 10.2L4.8 3.4L9.3 6.9Q12 6.1 14.7 6.9L19.2 3.4L19.8 10.2Q21 12.1 21 14.2C21 18.4 17.2 21 12 21C6.8 21 3 18.4 3 14.2Q3 12.1 4.2 10.2Z";

/// The brand mark (header logo): the cat, with open eyes.
pub fn brand_logo() -> String {
    svg(&cat_open())
}

fn cat_open() -> String {
    format!(
        "<path d='{CAT_HEAD}' fill='#000' stroke='none'/><ellipse cx='8.9' cy='14' rx='1.35' ry='1.6' fill='#fff' stroke='none'/><ellipse cx='15.1' cy='14' rx='1.35' ry='1.6' fill='#fff' stroke='none'/><path d='M11.2 17.1h1.6l-.8.9z' fill='#fff' stroke='#fff' stroke-width='.8'/>"
    )
}

/// Asleep (the core stopped): an outline, eyes closed.
fn cat_sleep() -> String {
    format!("<path d='{CAT_HEAD}' stroke-width='1.7'/><path d='M7.6 14.4q1.3 1.1 2.6 0M13.8 14.4q1.3 1.1 2.6 0' stroke-width='1.6'/>")
}

/// TUN on: the cat with narrowed, focused eyes.
fn cat_tun() -> String {
    format!("<path d='{CAT_HEAD}' fill='#000' stroke='none'/><path d='M7.6 13.9h2.7M13.7 13.9h2.7' stroke='#fff' stroke-width='1.7'/><path d='M11.2 17.1h1.6l-.8.9z' fill='#fff' stroke='#fff' stroke-width='.8'/>")
}

/// The menu bar marks as SVG documents (for the hand-drawn status item).
pub fn status_svg(running: bool, tun: bool) -> String {
    svg_doc(&match (running, tun) {
        (false, _) => cat_sleep(),
        (true, true) => cat_tun(),
        (true, false) => cat_open(),
    })
}

pub fn status_run() -> String {
    svg(&cat_open())
}

pub fn status_sleep() -> String {
    svg(&cat_sleep())
}

pub fn status_tun() -> String {
    svg(&cat_tun())
}
