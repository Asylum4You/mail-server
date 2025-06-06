/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs Ltd <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use utils::config::Config;

#[derive(Debug, Clone, Default)]
pub struct GroupwareConfig {
    // DAV settings
    pub max_request_size: usize,
    pub dead_property_size: Option<usize>,
    pub live_property_size: usize,
    pub max_lock_timeout: u64,
    pub max_locks_per_user: usize,
    pub max_results: usize,

    // Calendar settings
    pub max_ical_size: usize,
    pub max_ical_instances: usize,
    pub max_ical_attendees_per_instance: usize,
    pub default_calendar_name: Option<String>,
    pub default_calendar_display_name: Option<String>,

    // Addressbook settings
    pub max_vcard_size: usize,
    pub default_addressbook_name: Option<String>,
    pub default_addressbook_display_name: Option<String>,

    // File storage settings
    pub max_file_size: usize,
}

impl GroupwareConfig {
    pub fn parse(config: &mut Config) -> Self {
        GroupwareConfig {
            max_request_size: config
                .property("dav.request.max-size")
                .unwrap_or(25 * 1024 * 1024),
            dead_property_size: config
                .property_or_default::<Option<usize>>("dav.property.max-size.dead", "1024")
                .unwrap_or(Some(1024)),
            live_property_size: config.property("dav.property.max-size.live").unwrap_or(250),
            max_lock_timeout: config.property("dav.lock.max-timeout").unwrap_or(3600),
            max_locks_per_user: config.property("dav.locks.max-per-user").unwrap_or(10),
            max_results: config.property("dav.response.max-results").unwrap_or(2000),
            max_vcard_size: config.property("contacts.max-size").unwrap_or(512 * 1024),
            max_ical_size: config.property("calendar.max-size").unwrap_or(512 * 1024),
            default_calendar_name: config
                .property_or_default::<Option<String>>("calendar.default.href-name", "default")
                .unwrap_or_default(),
            default_addressbook_name: config
                .property_or_default::<Option<String>>("contacts.default.href-name", "default")
                .unwrap_or_default(),
            default_calendar_display_name: config
                .property_or_default::<Option<String>>(
                    "calendar.default.display-name",
                    "Stalwart Calendar",
                )
                .unwrap_or_default(),
            default_addressbook_display_name: config
                .property_or_default::<Option<String>>(
                    "contacts.default.display-name",
                    "Stalwart Address Book",
                )
                .unwrap_or_default(),
            max_ical_instances: config
                .property("calendar.max-recurrence-expansions")
                .unwrap_or(3000),
            max_ical_attendees_per_instance: config
                .property("calendar.max-attendees-per-instance")
                .unwrap_or(20),
            max_file_size: config
                .property("file-storage.max-size")
                .unwrap_or(25 * 1024 * 1024),
        }
    }
}
