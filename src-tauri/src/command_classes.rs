//! Where each app command runs when another computer drives this station.
//!
//! The remote client is this same app on another computer, with its own radio off, sending the
//! commands its screens make to the shack. Every command `lib.rs` registers in
//! `generate_handler!` gets exactly one class here. "Here" is the computer the operator sits at;
//! "the shack" is the one with the radio.
//!
//! The dispatcher (`command_dispatch`) reads it at both ends: the other computer routes each
//! command by its class, the shack admits each one by its class again, and a command with no row is
//! refused at both. The rules each class carries are the dispatcher's to enforce, not this file's.
//! What the table holds down is the list itself: the tests below fail on a registered command with
//! no row and on a row for a command that is not registered, so a new command cannot arrive
//! unclassed and a removed one cannot leave a row behind.

/// Where a command runs when another computer drives the shack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    /// At the shack: the radio and the cockpits, decodes, the logbook, settings, uploads, and
    /// imports and exports carried as text.
    RunsAtShack,
    /// On the computer the operator sits at: its windows, web links, notifications, the files it
    /// saves, its street-map packs and its own updater.
    RunsHere,
    /// Only from the shack's own screen: credentials, remote-access admin, the shack's own folders
    /// and file manager, and the commands whose reason below says why not from afar.
    RefusedHere,
    /// Can put the transmitter on the air, now or at a coming slot: a send, a CQ, a reply, a key,
    /// a tune carrier, or an arm that lets the station transmit without another press.
    StartsTx,
    /// Ends a transmission.
    StopsTx,
}

use Class::{RefusedHere, RunsAtShack, RunsHere, StartsTx, StopsTx};

/// One row per registered command, in name order: the command, its class, and why.
#[rustfmt::skip]
const CLASSES: &[(&str, Class, &str)] = &[
    ("add_radio",                      RunsAtShack, "Adds a radio to the shack's roster."),
    ("all_txt_location",               RunsAtShack, "The path of the shack's decode log, for display."),
    ("amp_command",                    RunsAtShack, "Band up, band down or operate on the shack's amplifier; keys nothing."),
    ("answer_remote_autostart_offer",  RefusedHere, "Part of turning on hosted Remote; remote-access admin stays at the shack."),
    ("app_version",                    RunsHere,    "This computer's version; both ends run the same build."),
    ("apply_park_states",              RunsAtShack, "Gives ticked contacts in the shack's logbook their park's state."),
    ("aprs_arm",                       StartsTx,    "Arming by hand allows the unattended APRS auto-ack."),
    ("aprs_auto_arm",                  RunsAtShack, "Receive-only arm on entering the APRS view; never allows the auto-ack."),
    ("aprs_send_beacon",               StartsTx,    "Queues an APRS beacon to transmit."),
    ("aprs_send_message",              StartsTx,    "Queues an APRS message to transmit."),
    ("aprs_tune",                      RunsAtShack, "Tunes the shack's rig for APRS; keys nothing."),
    ("archive_conversation",           RunsAtShack, "Deletes a chat thread and drops its queued messages."),
    ("arm_stream_mic",                 RefusedHere, "The stream's microphone; another computer's PTT is a held press of its own."),
    ("ask_log",                        RunsAtShack, "A question about the shack's logbook."),
    ("atu_tune",                       StartsTx,    "The rig's own tuner puts a carrier on the air."),
    ("broadcast",                      StartsTx,    "Queues an open broadcast message to transmit."),
    ("call_cq",                        StartsTx,    "Calls CQ and arms transmit."),
    ("call_station",                   StartsTx,    "Answers a decoded station; the sequencer transmits."),
    ("cancel_voice_recording",         RefusedHere, "Records from the shack's audio input; record here and import instead."),
    ("check_beta_update",              RunsHere,    "This computer's own updater."),
    ("check_for_update",               RunsHere,    "This computer's own updater."),
    ("choose_radio",                   RefusedHere, "Relaunches the shack bound to a radio: a restart of the station."),
    ("civ_diagnostic_log",             RunsAtShack, "Turns the shack's CI-V bus log on or off; the file stays at the shack."),
    ("civ_diagnostic_status",          RunsAtShack, "The shack's CI-V bus log state."),
    ("clear_activation",               RunsAtShack, "Ends the shack's current activation."),
    ("clear_cloudlog_key",             RefusedHere, "Credentials are set at the shack only."),
    ("clear_clublog_password",         RefusedHere, "Credentials are set at the shack only."),
    ("clear_data_folder",              RefusedHere, "Moves the shack's data folder at its next launch."),
    ("clear_eqsl_password",            RefusedHere, "Credentials are set at the shack only."),
    ("clear_hamqth_password",          RefusedHere, "Credentials are set at the shack only."),
    ("clear_hrdlog_code",              RefusedHere, "Credentials are set at the shack only."),
    ("clear_hunt_target",              RunsAtShack, "Clears the shack's hunt target."),
    ("clear_lotw_password",            RefusedHere, "Credentials are set at the shack only."),
    ("clear_qrz_logbook_key",          RefusedHere, "Credentials are set at the shack only."),
    ("clear_qrz_password",             RefusedHere, "Credentials are set at the shack only."),
    ("clear_voice_message",            RunsAtShack, "Clears one of the shack's voice-keyer slots."),
    ("clear_winlink_password",         RefusedHere, "Credentials are set at the shack only."),
    ("clear_wrl_key",                  RefusedHere, "Credentials are set at the shack only."),
    ("close_panel_window",             RunsHere,    "A window on this computer."),
    ("cloudlog_station_info",          RefusedHere, "Spends the stored Cloudlog key on connector setup, kept at the shack."),
    ("clublog_push_qso",               RunsAtShack, "An upload from the shack, with its stored credential."),
    ("confirm_pending_log",            RunsAtShack, "Logs the contact the prompt showed, by its key."),
    ("confirm_sat_uplink",             RunsAtShack, "Records the shack's satellite VFO mapping; keys nothing."),
    ("connect_web_status",             RunsAtShack, "The shack's Connect web page state."),
    ("contest_entry_reset",            RunsAtShack, "Clears the shack's contest entry line."),
    ("contest_i_moved",                RunsAtShack, "Records a move of the station during the contest session."),
    ("contest_log_manual",             RunsAtShack, "Logs a contest contact at the shack."),
    ("contest_log_satellite",          RunsAtShack, "Logs a contest contact made through a satellite at the shack."),
    ("contest_working",                RunsAtShack, "Binds a contest serial to a station at the shack."),
    ("contest_zone_hint",              RunsAtShack, "From the shack's country file."),
    ("cw_clear",                       RunsAtShack, "Clears the shack's CW decoder transcript."),
    ("cw_decode",                      RunsAtShack, "The shack's CW decoder."),
    ("cw_skim",                        RunsAtShack, "The shack's CW skimmer."),
    ("delete_qso",                     RunsAtShack, "Deletes one contact from the shack's logbook."),
    ("delete_qso_by_id",               RunsAtShack, "Deletes one contact from the shack's logbook."),
    ("detect_rigs",                    RunsAtShack, "Finds radios on the shack's USB ports."),
    ("diag_log_location",              RunsHere,    "This computer's own diagnostic log."),
    ("discard_pending_log",            RunsAtShack, "Discards the contact the prompt showed, by its key."),
    ("discover_flex",                  RunsAtShack, "Listens for a FlexRadio on the shack's network."),
    ("dismiss_tx_alarm",               RunsAtShack, "Clears one transmitter alarm banner; keys nothing."),
    ("display_metrics",                RunsHere,    "This computer's display."),
    ("dock_bandmap_window",            RunsHere,    "A window on this computer."),
    ("download_eqsl_report",           RunsAtShack, "A download at the shack, with its stored credential."),
    ("download_lotw_report",           RunsAtShack, "A download at the shack, with its stored credential."),
    ("download_parks",                 RunsAtShack, "Refreshes the shack's park list."),
    ("dxcc_entity_continents",         RunsAtShack, "From the shack's country file."),
    ("dxcc_entity_locations",          RunsAtShack, "From the shack's country file."),
    ("dxcc_entity_names",              RunsAtShack, "From the shack's country file."),
    ("edit_qso",                       RunsAtShack, "Edits one contact in the shack's logbook."),
    ("edit_qso_by_id",                 RunsAtShack, "Edits one contact in the shack's logbook."),
    ("eqsl_push_qso",                  RunsAtShack, "An upload from the shack, with its stored credential."),
    ("export_channels",                RunsAtShack, "A channel list as text; this computer saves it."),
    ("export_general_log",             RunsAtShack, "The shack's logbook as text; this computer saves it."),
    ("export_log",                     RunsAtShack, "The shack's contest log as text; this computer saves it."),
    ("export_log_for_activation",      RunsAtShack, "One activation's log as text; this computer saves it."),
    ("export_log_for_operator",        RunsAtShack, "One operator's log as text; this computer saves it."),
    ("export_settings_bundle",         RunsAtShack, "The shack's settings as text, with no secrets; this computer saves it."),
    ("fd_club_export",                 RunsAtShack, "The club log as text; this computer saves it."),
    ("fd_discover_events",             RunsAtShack, "Listens for club events on the shack's network."),
    ("fd_log_manual",                  RunsAtShack, "Logs a Field Day contact at the shack."),
    ("fd_merge_to_general",            RunsAtShack, "Merges the contest session into the shack's logbook."),
    ("fd_scoreboard_status",           RunsAtShack, "The shack's scoreboard page state."),
    ("fd_set_upload",                  RunsAtShack, "The contest session's upload destinations at the shack."),
    ("fetch_cty",                      RunsAtShack, "Updates the shack's country file."),
    ("fetch_fcc_states",               RunsAtShack, "Updates the shack's callsign database."),
    ("fetch_fd_rules",                 RunsAtShack, "Updates the shack's Field Day rules."),
    ("fetch_lotw_users",               RunsAtShack, "Updates the shack's LoTW user list."),
    ("fetch_tles_now",                 RunsAtShack, "Updates the shack's satellite elements."),
    ("geocode_city",                   RunsAtShack, "A city search from the shack, for its location."),
    ("get_activation",                 RunsAtShack, "The shack's activation."),
    ("get_all_rig_models",             RunsAtShack, "The rig catalog, for the shack's rig setup."),
    ("get_all_spots",                  RunsAtShack, "The shack's spots."),
    ("get_aprs_health",                RunsAtShack, "The shack's APRS decoder."),
    ("get_aprs_heard",                 RunsAtShack, "The shack's APRS decoder."),
    ("get_aprs_is_status",             RunsAtShack, "The shack's APRS-IS feed."),
    ("get_aprs_stations",              RunsAtShack, "The shack's APRS stations."),
    ("get_aprs_tx_notice",             RunsAtShack, "Why the shack last dropped an APRS frame."),
    ("get_assistance_journal",         RunsAtShack, "The shack's assistance journal."),
    ("get_audio_devices",              RunsAtShack, "The shack's sound cards, for its rig audio."),
    ("get_aurora",                     RunsAtShack, "The shack's propagation feeds."),
    ("get_awards",                     RunsAtShack, "From the shack's logbook."),
    ("get_band_outlook",               RunsAtShack, "The shack's propagation model."),
    ("get_band_plan",                  RunsAtShack, "The shack's band plan."),
    ("get_cat_cw_unproven_rig_models", RunsAtShack, "The rig catalog, for the shack's rig setup."),
    ("get_cluster_nodes",              RunsAtShack, "The shack's DX-cluster nodes."),
    ("get_confirmation_diagnostics",   RunsAtShack, "From the shack's logbook."),
    ("get_connection_log",             RefusedHere, "Its lines carry the services' own words, which have carried keys."),
    ("get_contests",                   RunsAtShack, "The shack's contest calendar."),
    ("get_credentials_status",         RefusedHere, "Reads the shack's keychain; credentials are managed at the shack only."),
    ("get_cty_status",                 RunsAtShack, "The shack's country file."),
    ("get_data_folder",                RunsAtShack, "The shack's data folder, for display."),
    ("get_declination",                RunsAtShack, "At the shack's location."),
    ("get_dxped_windows",              RunsAtShack, "From the shack's location."),
    ("get_fast_power",                 RunsAtShack, "The shack's meters."),
    ("get_fcc_states_status",          RunsAtShack, "The shack's callsign database."),
    ("get_fd_rules_status",            RunsAtShack, "The shack's Field Day rules."),
    ("get_fd_ruleset",                 RunsAtShack, "The shack's Field Day rules."),
    ("get_feed_health",                RunsAtShack, "The shack's live feeds."),
    ("get_getting_out",                RunsAtShack, "Who is hearing the shack."),
    ("get_iss_pass",                   RunsAtShack, "From the shack's location."),
    ("get_journey",                    RunsAtShack, "From the shack's logbook."),
    ("get_js8_state",                  RunsAtShack, "The shack's JS8 state."),
    ("get_kc2g_muf",                   RunsAtShack, "The shack's propagation feeds."),
    ("get_kp_forecast",                RunsAtShack, "The shack's propagation feeds."),
    ("get_licensed_band_plan",         RunsAtShack, "The shack's licence and band plan."),
    ("get_log_stats",                  RunsAtShack, "From the shack's logbook."),
    ("get_lotw_users_status",          RunsAtShack, "The shack's LoTW user list."),
    ("get_meters",                     RunsAtShack, "The shack's meters."),
    ("get_need_alerts",                RunsAtShack, "From the shack's logbook and spots."),
    ("get_openings_log",               RunsAtShack, "The shack's openings history."),
    ("get_ota_map_spots",              RunsAtShack, "The shack's activator spots."),
    ("get_ota_spots",                  RunsAtShack, "The shack's activator spots."),
    ("get_path_outlook",               RunsAtShack, "The shack's propagation model."),
    ("get_pca",                        RunsAtShack, "The shack's propagation feeds."),
    ("get_portless_rig_models",        RunsAtShack, "The rig catalog, for the shack's rig setup."),
    ("get_privilege_spans",            RunsAtShack, "The shack's licence edges."),
    ("get_propagation",                RunsAtShack, "The shack's propagation picture."),
    ("get_psk_state",                  RunsAtShack, "The shack's PSK state."),
    ("get_ptt_mic_data_rig_models",    RunsAtShack, "The rig catalog, for the shack's rig setup."),
    ("get_remote_monitor_frame",       RunsAtShack, "A bounded read of the shack's state; only a development view calls it."),
    ("get_remote_station_status",      RefusedHere, "Remote-access admin stays at the shack."),
    ("get_rf_frame",                   RunsAtShack, "The shack's panadapter."),
    ("get_rig_models",                 RunsAtShack, "The rig catalog, for the shack's rig setup."),
    ("get_rtty_state",                 RunsAtShack, "The shack's RTTY state."),
    ("get_sat_detail",                 RunsAtShack, "The shack's satellite picture."),
    ("get_sat_pass_needs",             RunsAtShack, "From the shack's location and logbook."),
    ("get_sat_schedule",               RunsAtShack, "From the shack's location."),
    ("get_sat_sked",                   RunsAtShack, "From the shack's location."),
    ("get_sat_transponder",            RunsAtShack, "The shack's satellite picture."),
    ("get_satellites",                 RunsAtShack, "From the shack's location."),
    ("get_scope_frame",                RunsAtShack, "The shack's scope."),
    ("get_scope_row",                  RunsAtShack, "The shack's scope."),
    ("get_serial_ports",               RunsAtShack, "The shack's serial ports, for its rig setup."),
    ("get_serial_ports_detailed",      RunsAtShack, "The shack's serial ports, for its rig setup."),
    ("get_settings",                   RunsAtShack, "The shack's settings."),
    ("get_snapshot",                   RunsAtShack, "The shack's state."),
    ("get_solar_indices",              RunsAtShack, "The shack's propagation feeds."),
    ("get_space_wx_scales",            RunsAtShack, "The shack's propagation feeds."),
    ("get_spectrum_row",               RunsAtShack, "The shack's waterfall."),
    ("get_sstv_state",                 RunsAtShack, "The shack's SSTV state."),
    ("get_tle_status",                 RunsAtShack, "The shack's satellite elements."),
    ("get_voice_messages",             RunsAtShack, "The shack's voice-keyer slots."),
    ("get_window_behind",              RunsHere,    "A window on this computer."),
    ("get_xray_now",                   RunsAtShack, "The shack's propagation feeds."),
    ("halt_tx",                        StopsTx,     "Stops transmitting now and drops what is queued."),
    ("hrdlog_push_qso",                RunsAtShack, "An upload from the shack, with its stored credential."),
    ("hunted_parks_count",             RunsAtShack, "The shack's hunted parks."),
    ("import_adif",                    RunsAtShack, "A log file's text, imported into the shack's logbook."),
    ("import_hunted_parks_csv",        RunsAtShack, "A file's text, imported at the shack."),
    ("import_parks_csv",               RunsAtShack, "A file's text, imported at the shack."),
    ("import_pota_log",                RunsAtShack, "A file's text, imported at the shack."),
    ("import_settings_bundle",         RefusedHere, "Replaces the shack's whole configuration from a file."),
    ("import_tles",                    RunsAtShack, "A file's text, imported at the shack."),
    ("import_voice_message",           RunsAtShack, "A message recorded here, sent as audio to one of the shack's slots."),
    ("install_beta_update",            RunsHere,    "This computer's own updater."),
    ("js8_answer_reply",               StartsTx,    "A yes queues the automatic reply to transmit."),
    ("js8_arm",                        StartsTx,    "Arms an unattended JS8 path: autoreply, relay, heartbeat or its ack."),
    ("js8_call_cq",                    StartsTx,    "Calls CQ on JS8."),
    ("js8_composer",                   RunsAtShack, "The shack's JS8 compose box; keys nothing."),
    ("js8_cq_repeat",                  StartsTx,    "Arms the repeating JS8 CQ."),
    ("js8_drop_queue",                 RunsAtShack, "Drops the JS8 outbox; a sender control, while Stop is halt_tx."),
    ("js8_enter",                      RunsAtShack, "Entering the JS8 view; receive only."),
    ("js8_inbox_delete",               RunsAtShack, "The shack's JS8 inbox."),
    ("js8_inbox_mark",                 RunsAtShack, "The shack's JS8 inbox."),
    ("js8_locator_refusal",            RunsAtShack, "The shack's JS8 locator gate."),
    ("js8_send",                       StartsTx,    "Queues a JS8 message to transmit."),
    ("js8_send_command",               StartsTx,    "Queues a JS8 command to transmit."),
    ("js8_set_rx_speeds",              RunsAtShack, "The shack's JS8 receive speeds."),
    ("js8_set_speed",                  RunsAtShack, "The shack's JS8 transmit speed; keys nothing."),
    ("log_activations",                RunsAtShack, "From the shack's logbook."),
    ("log_current_qso",                RunsAtShack, "Logs the current contact at the shack."),
    ("log_operators",                  RunsAtShack, "From the shack's logbook."),
    ("log_qso",                        RunsAtShack, "Logs a contact at the shack."),
    ("logbook_save_choice",            RunsHere,    "This computer's own quit-time question."),
    ("lookup_park",                    RunsAtShack, "The shack's park list."),
    ("lookup_park_live",               RunsAtShack, "A park lookup from the shack."),
    ("lotw_sat_names",                 RunsAtShack, "The satellite names the shack's logbook accepts."),
    ("mark_lotw_uploaded",             RunsAtShack, "Marks contacts in the shack's logbook."),
    ("mark_qsl_card",                  RunsAtShack, "Marks one contact in the shack's logbook."),
    ("mark_qsl_card_by_id",            RunsAtShack, "Marks one contact in the shack's logbook."),
    ("mark_qsl_sent",                  RunsAtShack, "Marks one contact in the shack's logbook."),
    ("mark_qsl_sent_by_id",            RunsAtShack, "Marks one contact in the shack's logbook."),
    ("n3fjp_test_connection",          RefusedHere, "Logbook connector setup stays at the shack."),
    ("notify_erase",                   RunsAtShack, "Tells the shack's companion apps a pane was cleared."),
    ("open_download_page",             RunsHere,    "Opens a web page in this computer's browser."),
    ("open_dxped_page",                RunsHere,    "Opens an http or https page in this computer's browser."),
    ("open_external_url",              RunsHere,    "Opens an http or https page in this computer's browser."),
    ("open_lan_stations_window",       RunsHere,    "This computer's window onto stations on its network."),
    ("open_panel_window",              RunsHere,    "A window on this computer."),
    ("open_qrz_page",                  RunsHere,    "Opens a web page in this computer's browser."),
    ("open_remote_stations_window",    RunsHere,    "This computer's window onto hosted Remote."),
    ("os_notify",                      RunsHere,    "A notification on this computer."),
    ("override_next_tx",               StartsTx,    "Chooses the next FT transmission and arms it."),
    ("park_state_review",              RunsAtShack, "Reads the shack's logbook for contacts whose park names another state."),
    ("parks_count",                    RunsAtShack, "The shack's park list."),
    ("pick_band",                      RunsAtShack, "Changes the shack's band; keys nothing."),
    ("pick_data_folder",               RefusedHere, "Opens a folder dialog on the shack's own screen."),
    ("play_voice_message",             StartsTx,    "Plays a voice-keyer message on the air."),
    ("point_rotator",                  RunsAtShack, "Turns the shack's rotator."),
    ("point_rotator_at_call",          RunsAtShack, "Turns the shack's rotator."),
    ("point_rotator_elevation",        RunsAtShack, "Turns the shack's rotator."),
    ("post_spot",                      RunsAtShack, "Posts a spot to the DX cluster from the shack."),
    ("prepare_update_install",         RunsHere,    "This computer's own updater."),
    ("preview_cw",                     RunsAtShack, "Expands a CW macro at the shack; sends nothing."),
    ("probe_cat_ports",                RunsAtShack, "Probes the shack's serial ports for the rig; never transmits."),
    ("psk_afc_reset",                  RunsAtShack, "The shack's PSK decoder."),
    ("psk_arm",                        RunsAtShack, "The shack's PSK decoder; receive only."),
    ("psk_auto_arm",                   RunsAtShack, "The shack's PSK decoder; receive only."),
    ("psk_clear",                      RunsAtShack, "The shack's PSK transcript."),
    ("psk_net",                        RunsAtShack, "The shack's PSK decoder."),
    ("psk_send",                       StartsTx,    "Queues PSK text to transmit."),
    ("psk_set_latched",                StartsTx,    "Keys continuous PSK transmit."),
    ("psk_set_mode",                   RunsAtShack, "The shack's PSK sub-mode."),
    ("psk_stop",                       StopsTx,     "Stops PSK transmit now."),
    ("psk_type",                       StartsTx,    "Types into a live PSK transmission."),
    ("publish_remote_memory_bank",     RefusedHere, "Feeds hosted Remote from the shack's own main window."),
    ("purge_log",                      RefusedHere, "Deletes every contact in the shack's logbook, irreversibly."),
    ("qrz_correct_apply",              RunsAtShack, "Sends one correction to QRZ from the shack."),
    ("qrz_correct_preview",            RunsAtShack, "Reads QRZ's copy of one contact from the shack."),
    ("qrz_correct_undo_miss",          RunsAtShack, "Undoes the shack's own missed replace at QRZ."),
    ("qrz_lookup",                     RunsAtShack, "A callbook lookup from the shack."),
    ("qrz_push_qso",                   RunsAtShack, "An upload from the shack, with its stored credential."),
    ("qrz_test_connection",            RefusedHere, "Checks the stored QRZ key; credentials are managed at the shack only."),
    ("qso_freetext",                   StartsTx,    "Sets the text of the next FT transmission."),
    ("qso_resend",                     StartsTx,    "Sends the current FT message again at the next slot."),
    ("qsy_configure",                  RunsAtShack, "The shack's QSY channel plan; keys nothing."),
    ("qsy_move_now",                   RunsAtShack, "Announces a move on the next over; starts no over."),
    ("qsy_pause",                      RunsAtShack, "Holds or resumes QSY hopping."),
    ("qsy_set_enabled",                RunsAtShack, "Turns coordinated QSY on or off; starts no over."),
    ("qsy_stop",                       RunsAtShack, "Ends coordinated QSY and returns home; not a transmit stop."),
    ("radio_launch_info",              RunsAtShack, "The shack's radio roster."),
    ("radioprog_delete_project",       RunsAtShack, "The shack's radio-programming projects."),
    ("radioprog_file_notice",          RunsAtShack, "The shack's radio-programming projects."),
    ("radioprog_list_projects",        RunsAtShack, "The shack's radio-programming projects."),
    ("radioprog_save_project",         RunsAtShack, "The shack's radio-programming projects."),
    ("read_rotator",                   RunsAtShack, "The shack's rotator."),
    ("read_rotator_state",             RunsAtShack, "The shack's rotator."),
    ("recordings_location",            RunsAtShack, "The path of the shack's recordings, for display."),
    ("redecode",                       RunsAtShack, "Runs the shack's decoder again."),
    ("release_stream_mic",             StopsTx,     "Ends a microphone over armed through the stream."),
    ("remote_station_action",          RefusedHere, "Remote-access admin stays at the shack."),
    ("remove_radio",                   RunsAtShack, "Removes a radio from the shack's roster."),
    ("rename_radio",                   RunsAtShack, "Renames a radio in the shack's roster."),
    ("repair_clock",                   RefusedHere, "Repairs the shack's clock through an administrator prompt that waits on the shack's own screen."),
    ("repeater_search",                RunsAtShack, "A repeater search from the shack."),
    ("repeater_tune",                  RunsAtShack, "Tunes the shack's rig to a repeater; keys nothing."),
    ("resend_chat",                    StartsTx,    "Queues a chat message to transmit again."),
    ("reset_lotw_cursor",              RunsAtShack, "Empties the shack's LoTW sync cursor, so its next download asks for everything."),
    ("reset_settings",                RefusedHere, "Replaces the shack's whole configuration with factory defaults."),
    ("resolve_entity",                 RunsAtShack, "From the shack's country file."),
    ("restart_app",                    RunsHere,    "Restarts this computer's Nexus to finish its update."),
    ("resume_chat_cq",                 StartsTx,    "Resumes the chat CQ run."),
    ("retire_wanted_calls",            RunsAtShack, "Operator data kept at the shack."),
    ("reveal_all_txt",                 RefusedHere, "Opens the shack's file manager on its own screen."),
    ("reveal_diag_log",                RunsHere,    "Shows this computer's own diagnostic log."),
    ("reveal_recordings",              RefusedHere, "Opens the shack's file manager on its own screen."),
    ("reveal_sstv_gallery",            RefusedHere, "Opens the shack's file manager on its own screen."),
    ("route_preview",                  RunsAtShack, "The shack's band routing; moves no rig."),
    ("rtty_afc_reset",                 RunsAtShack, "The shack's RTTY decoder."),
    ("rtty_arm",                       RunsAtShack, "The shack's RTTY decoder; receive only."),
    ("rtty_auto_abort",                StopsTx,     "Ends the RTTY auto session and unkeys."),
    ("rtty_auto_answer",               StartsTx,    "Answers a CQ; the RTTY sequencer transmits."),
    ("rtty_auto_arm",                  RunsAtShack, "The shack's RTTY decoder; receive only."),
    ("rtty_auto_cq",                   StartsTx,    "Starts an RTTY CQ run."),
    ("rtty_clear",                     RunsAtShack, "The shack's RTTY transcript."),
    ("rtty_net",                       RunsAtShack, "The shack's RTTY decoder."),
    ("rtty_send",                      StartsTx,    "Queues RTTY text to transmit."),
    ("rtty_set_auto",                  RunsAtShack, "The RTTY sequencer on or off; it never keys, and off ends a session."),
    ("rtty_set_latched",               StartsTx,    "Keys continuous RTTY transmit."),
    ("rtty_stop",                      StopsTx,     "Stops RTTY transmit now."),
    ("rtty_type",                      StartsTx,    "Types into a live RTTY transmission."),
    ("sat_track_status",               RunsAtShack, "The shack's satellite tracking."),
    ("save_png_to_downloads",          RunsHere,    "Saves a file in this computer's Downloads folder."),
    ("save_text_to_downloads",         RunsHere,    "Saves a file in this computer's Downloads folder."),
    ("search_parks",                   RunsAtShack, "The shack's park list."),
    ("select_peer",                    RunsAtShack, "Selects a chat conversation at the shack."),
    ("self_spot_activation",           RunsAtShack, "Posts the shack's activation spot."),
    ("send_cw",                        StartsTx,    "Queues CW to transmit."),
    ("send_message",                   StartsTx,    "Queues a chat message to transmit."),
    ("set_activation",                 RunsAtShack, "Begins an activation at the shack."),
    ("set_activation_state",           RunsAtShack, "Picks the state a park on a state line is activated from."),
    ("set_active_radio",               RunsAtShack, "Switches the shack's active radio; any carrier is dropped first."),
    ("set_af_gain",                    RunsAtShack, "The shack's rig control."),
    ("set_agc",                        RunsAtShack, "The shack's rig control."),
    ("set_ai_cw",                      RunsAtShack, "The shack's CW decoder."),
    ("set_area",                       RunsAtShack, "Chat or FT at the shack; lands on a passive mode."),
    ("set_att_db",                     RunsAtShack, "The shack's rig control."),
    ("set_beacon",                     StartsTx,    "The presence beacon transmits by itself, periodically."),
    ("set_beta_updates",               RunsHere,    "This computer's update channel."),
    ("set_blocked_calls",              RunsAtShack, "Operator data kept at the shack."),
    ("set_chat_cq",                    StartsTx,    "Starts the chat CQ run."),
    ("set_cloudlog_key",               RefusedHere, "Credentials are set at the shack only."),
    ("set_clublog_password",           RefusedHere, "Credentials are set at the shack only."),
    ("set_comp_level",                 RunsAtShack, "The shack's rig control."),
    ("set_cw_keyer",                   RunsAtShack, "The shack's CW keyer setup; keys nothing."),
    ("set_cw_peer_info",               RunsAtShack, "The shack's CW macro fields."),
    ("set_cw_wpm",                     RunsAtShack, "The shack's CW speed."),
    ("set_data_folder",                RefusedHere, "Moves the shack's data folder at its next launch."),
    ("set_decode_depth",               RunsAtShack, "The shack's FT decode depth."),
    ("set_default_radio",              RunsAtShack, "The shack's band routing."),
    ("set_eqsl_password",              RefusedHere, "Credentials are set at the shack only."),
    ("set_fd_operator",                RunsAtShack, "Who is at the key in the shack's contest session."),
    ("set_filter_width",               RunsAtShack, "The shack's rig control."),
    ("set_flex_pan_ref",               RunsAtShack, "The shack's FlexRadio panadapter."),
    ("set_flex_pan_span",              RunsAtShack, "The shack's FlexRadio panadapter."),
    ("set_frequency",                  RunsAtShack, "Tunes the shack's rig; keys nothing."),
    ("set_hamqth_password",            RefusedHere, "Credentials are set at the shack only."),
    ("set_hold_tx_freq",               RunsAtShack, "The shack's FT setting; keys nothing."),
    ("set_hrdlog_code",                RefusedHere, "Credentials are set at the shack only."),
    ("set_hunt_target",                RunsAtShack, "The shack's hunt target."),
    ("set_launch_at_login",            RunsHere,    "Starts Nexus when someone signs in to this computer."),
    ("set_license_class",              RunsAtShack, "The shack's licence class; its transmit gate stays at the shack."),
    ("set_log_form_grid",              RunsAtShack, "The shack's log form."),
    ("set_lotw_password",              RefusedHere, "Credentials are set at the shack only."),
    ("set_mic_gain",                   RunsAtShack, "The shack's rig control."),
    ("set_mode",                       StartsTx,    "A run mode calls CQ and arms transmit."),
    ("set_monitor_gain",               RunsAtShack, "The shack's rig control."),
    ("set_msk144_period",              RunsAtShack, "The shack's MSK144 period setting; keys nothing."),
    ("set_notch_freq",                 RunsAtShack, "The shack's rig control."),
    ("set_nr_level",                   RunsAtShack, "The shack's rig control."),
    ("set_operating_mode",             RunsAtShack, "The shack's section and rig mode; each transmission is its own press."),
    ("set_peg_lock",                   RunsAtShack, "The shack's band routing."),
    ("set_preamp_db",                  RunsAtShack, "The shack's rig control."),
    ("set_psk_macros",                 RunsAtShack, "The shack's PSK macros; sends nothing."),
    ("set_ptt",                        StartsTx,    "Keys the rig; from another computer it becomes a held press."),
    ("set_qrz_logbook_key",            RefusedHere, "Credentials are set at the shack only."),
    ("set_qrz_password",               RefusedHere, "Credentials are set at the shack only."),
    ("set_radio_bands",                RunsAtShack, "The shack's band routing."),
    ("set_repeaterbook_token",         RefusedHere, "Credentials are set at the shack only."),
    ("set_rf_gain",                    RunsAtShack, "The shack's rig control."),
    ("set_rf_power",                   RunsAtShack, "The shack's rig control."),
    ("set_rig_func",                   RunsAtShack, "The rig's NB, NR, notch, COMP, monitor and VOX switches, VOX on included."),
    ("set_rit",                        RunsAtShack, "The shack's rig control."),
    ("set_routing_rules",              RunsAtShack, "The shack's band routing."),
    ("set_rtty_macros",                RunsAtShack, "The shack's RTTY macros; sends nothing."),
    ("set_rx_gain",                    RunsAtShack, "The shack's receive audio."),
    ("set_rx_offset",                  RunsAtShack, "The shack's FT setting; keys nothing."),
    ("set_sat_tag",                    RunsAtShack, "Tags one contact in the shack's logbook."),
    ("set_sat_tag_by_id",              RunsAtShack, "Tags one contact in the shack's logbook."),
    ("set_sat_transponder",            RunsAtShack, "The shack's satellite tracking."),
    ("set_scope_fixed",                RunsAtShack, "The shack's scope."),
    ("set_scope_ref",                  RunsAtShack, "The shack's scope."),
    ("set_scope_span",                 RunsAtShack, "The shack's scope."),
    ("set_settings",                   RunsAtShack, "The shack's settings, saved whole."),
    ("set_sideband_override",          RunsAtShack, "The shack's rig control."),
    ("set_skip_tx1",                   RunsAtShack, "The shack's FT setting; keys nothing."),
    ("set_slice",                      RunsAtShack, "A FlexRadio slice at the shack that is not the transmit slice."),
    ("set_source",                     RunsAtShack, "The shack's receive source."),
    ("set_split",                      RunsAtShack, "The shack's rig control."),
    ("set_squelch",                    RunsAtShack, "The shack's rig control."),
    ("set_sub_level",                  RunsAtShack, "The shack's rig control."),
    ("set_tier",                       RunsAtShack, "The shack's FT mode; keys nothing."),
    ("set_tune",                       StartsTx,    "Puts a tune carrier on the air."),
    ("set_tx_cycle_auto",              RunsAtShack, "The shack's FT setting; keys nothing."),
    ("set_tx_enabled",                 StartsTx,    "Enable TX: the FT sequencer may key at the next slot."),
    ("set_tx_even",                    RunsAtShack, "The shack's FT setting; keys nothing."),
    ("set_tx_level",                   RunsAtShack, "The shack's rig control."),
    ("set_tx_offset",                  RunsAtShack, "The shack's FT setting; keys nothing."),
    ("set_unassisted_mode",            RunsAtShack, "The shack's contest entry."),
    ("set_vfo",                        RunsAtShack, "The shack's rig control."),
    ("set_voice_label",                RunsAtShack, "Names one of the shack's voice-keyer slots."),
    ("set_watch_list",                 RunsAtShack, "Operator data kept at the shack."),
    ("set_window_behind",              RunsHere,    "A window on this computer."),
    ("set_winlink_password",           RefusedHere, "Credentials are set at the shack only."),
    ("set_wrl_key",                    RefusedHere, "Credentials are set at the shack only."),
    ("set_xit",                        RunsAtShack, "The shack's rig control."),
    ("set_yaesu_scope_mode",           RunsAtShack, "The shack's scope."),
    ("sstv_arm",                       RunsAtShack, "The shack's SSTV decoder; receive only."),
    ("sstv_auto_arm",                  RunsAtShack, "The shack's SSTV decoder; receive only."),
    ("sstv_auto_disarm",               RunsAtShack, "The shack's SSTV decoder; receive only."),
    ("sstv_delete_image",              RunsAtShack, "Deletes one received picture inside the shack's gallery."),
    ("sstv_manual_rx",                 RunsAtShack, "The shack's SSTV decoder; receive only."),
    ("sstv_send",                      StartsTx,    "Transmits an SSTV picture."),
    ("sstv_stop",                      StopsTx,     "Stops the SSTV transmission now."),
    ("sstv_tune",                      RunsAtShack, "Tunes the shack's rig for SSTV; keys nothing."),
    ("start_cq",                       StartsTx,    "Starts a CQ run."),
    ("start_qso_recording",            RunsAtShack, "Records the shack's receive audio at the shack."),
    ("start_sat_track",                RunsAtShack, "The shack's satellite tracking."),
    ("start_voice_recording",          RefusedHere, "Records from the shack's audio input; record here and import instead."),
    ("stop_cw",                        StopsTx,     "Stops CW now."),
    ("stop_qso_recording",             RunsAtShack, "Records the shack's receive audio at the shack."),
    ("stop_rotator",                   RunsAtShack, "Stops the shack's rotator; not a transmit stop."),
    ("stop_sat_track",                 RunsAtShack, "The shack's satellite tracking."),
    ("stop_voice",                     StopsTx,     "Stops voice playback now and unkeys."),
    ("stop_voice_recording",           RefusedHere, "Records from the shack's audio input; record here and import instead."),
    ("street_map_asset",               RunsHere,    "This computer's street-map packs."),
    ("street_map_cancel",              RunsHere,    "This computer's street-map packs."),
    ("street_map_download",            RunsHere,    "This computer's street-map packs."),
    ("street_map_info",                RunsHere,    "This computer's street-map packs."),
    ("street_map_install_file",        RunsHere,    "A file picker and a street-map pack on this computer."),
    ("street_map_packs",               RunsHere,    "This computer's street-map packs."),
    ("street_map_read",                RunsHere,    "This computer's street-map packs."),
    ("street_map_remove",              RunsHere,    "This computer's street-map packs."),
    ("street_map_size",                RunsHere,    "This computer's street-map packs."),
    ("street_map_unfinished",          RunsHere,    "This computer's street-map packs."),
    ("street_map_updates",             RunsHere,    "This computer's street-map packs."),
    ("swap_vfo",                       RunsAtShack, "The shack's rig control."),
    ("sync_lotw_report",               RunsAtShack, "A report's text, reconciled into the shack's logbook."),
    ("sync_qrz",                       RunsAtShack, "A sync at the shack, with its stored credential."),
    ("test_cat",                       RunsAtShack, "Tests the shack's rig connection."),
    ("tune_channel",                   RunsAtShack, "Tunes the shack's rig; keys nothing."),
    ("ui_state_load",                  RunsAtShack, "Operator data kept at the shack."),
    ("ui_state_save",                  RunsAtShack, "Operator data kept at the shack."),
    ("update_install_block",           RunsAtShack, "Whether the shack's radio is busy, which an update here waits on."),
    ("update_radio_profile",           RunsAtShack, "One radio's setup at the shack, saved whole."),
    ("update_route",                   RunsHere,    "How this computer's Nexus takes an update."),
    ("upload_lotw_report",             RunsAtShack, "Signs and uploads at the shack, with its own TQSL."),
    ("use_single_radio",               RefusedHere, "The shack's launch picker, a choice made as the station starts."),
    ("winlink_connect",                RunsAtShack, "A CMS session from the shack with its stored password; a typed one is refused."),
    ("winlink_disconnect",             RunsAtShack, "Ends the shack's Winlink session; not a transmit stop."),
    ("winlink_mailbox_list",           RunsAtShack, "The shack's Winlink mailbox."),
    ("winlink_mailbox_location",       RunsAtShack, "The path of the shack's Winlink mailbox, for display."),
    ("winlink_mailbox_rebuild",        RunsAtShack, "The shack's Winlink mailbox."),
    ("winlink_mark_read",              RunsAtShack, "The shack's Winlink mailbox."),
    ("winlink_message",                RunsAtShack, "The shack's Winlink mailbox."),
    ("winlink_session_status",         RunsAtShack, "The shack's Winlink session."),
    ("work_spot",                      RunsAtShack, "QSY to a spot with its mode and split; keys nothing."),
    ("wrl_push_qso",                   RunsAtShack, "An upload from the shack, with its stored credential."),
];

/// The class of the command `name`, as the app invokes it (`edit_qso_by_id`, never its module
/// path), or `None` when it has no row, which both ends refuse. The rows are in name order, which
/// a test below holds, so this is a binary search.
pub(crate) fn class_of(name: &str) -> Option<Class> {
    CLASSES
        .binary_search_by(|(row, ..)| (*row).cmp(name))
        .ok()
        .map(|at| CLASSES[at].1)
}

/// Every command and its class, in name order: what the build's contract is made from.
pub(crate) fn classes() -> impl Iterator<Item = (&'static str, Class)> {
    CLASSES.iter().map(|(name, class, _)| (*name, *class))
}

/// The command names `generate_handler!` registers, with any module path dropped
/// (`log_by_id::edit_qso_by_id` is `edit_qso_by_id`). It reads `lib.rs` as text, as the other
/// registration tests do, because the list exists nowhere else.
#[cfg(test)]
pub(crate) fn registered() -> Vec<&'static str> {
    include_str!("lib.rs")
        .split_once("tauri::generate_handler![")
        .and_then(|(_, rest)| rest.split_once("])"))
        .map(|(list, _)| list)
        .expect("lib.rs has a generate_handler! list")
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            // A path is all this reads. An attribute or a comment in the list fails here, by
            // name, instead of passing as a command that nothing registered.
            assert!(
                entry
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':'),
                "a handler entry this test cannot read: {entry:?}"
            );
            entry.rsplit("::").next().unwrap_or(entry)
        })
        .collect()
}

#[test]
fn every_registered_command_has_a_class() {
    let missing: Vec<&str> = registered()
        .into_iter()
        .filter(|name| !CLASSES.iter().any(|(row, ..)| row == name))
        .collect();
    assert!(
        missing.is_empty(),
        "no class for {missing:?}: add one row each to CLASSES in command_classes.rs"
    );
}

#[test]
fn every_row_names_a_registered_command() {
    let registered = registered();
    let stale: Vec<&str> = CLASSES
        .iter()
        .map(|(row, ..)| *row)
        .filter(|row| !registered.contains(row))
        .collect();
    assert!(
        stale.is_empty(),
        "rows for commands lib.rs does not register: {stale:?}"
    );
}

/// Exactly one class each: a second row for a command fails here, as does a row out of place.
#[test]
fn one_row_per_command_in_name_order() {
    for pair in CLASSES.windows(2) {
        assert!(
            pair[0].0 < pair[1].0,
            "{:?} follows {:?}: one row per command, in name order",
            pair[1].0,
            pair[0].0
        );
    }
}

/// The lookup the dispatcher routes and admits by finds every row with its own class, and nothing
/// that has no row.
#[test]
fn class_of_finds_every_row_and_nothing_else() {
    for (name, class, _) in CLASSES {
        assert_eq!(class_of(name), Some(*class), "{name}");
    }
    for name in [
        "",
        "throwaway_unclassed_probe",
        "zzz",
        "log_by_id::edit_qso_by_id",
    ] {
        assert_eq!(class_of(name), None, "{name:?}");
    }
}
