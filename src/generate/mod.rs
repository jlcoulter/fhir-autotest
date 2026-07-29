pub mod bulk_data;
pub mod conformance;
pub mod dependency_resolver;
pub mod model;
pub mod planner;
pub mod resource_generator;

pub use bulk_data::*;
pub use conformance::*;
pub use dependency_resolver::*;
pub use model::*;
pub use planner::*;
pub use resource_generator::*;

// ---------------------------------------------------------------------------
// Australian locality data shared across generators
// ---------------------------------------------------------------------------

pub struct AuLocality {
    pub suburb: &'static str,
    pub city: &'static str,
    pub state: &'static str,
    pub postcode: &'static str,
    /// Approximate WGS84 coordinates
    pub lat: f64,
    pub lon: f64,
}

/// A representative set of Australian localities covering all states/territories.
/// Each entry is a real suburb or town with its state, postcode, and centre coordinates.
pub static AU_LOCALITIES: &[AuLocality] = &[
    // NSW
    AuLocality { suburb: "Sydney",         city: "Sydney",        state: "NSW", postcode: "2000", lat: -33.8688, lon: 151.2093 },
    AuLocality { suburb: "Parramatta",     city: "Parramatta",    state: "NSW", postcode: "2150", lat: -33.8150, lon: 151.0011 },
    AuLocality { suburb: "Penrith",        city: "Penrith",       state: "NSW", postcode: "2750", lat: -33.7512, lon: 150.6942 },
    AuLocality { suburb: "Newcastle",      city: "Newcastle",     state: "NSW", postcode: "2300", lat: -32.9283, lon: 151.7817 },
    AuLocality { suburb: "Wollongong",     city: "Wollongong",    state: "NSW", postcode: "2500", lat: -34.4278, lon: 150.8931 },
    AuLocality { suburb: "Coffs Harbour",  city: "Coffs Harbour", state: "NSW", postcode: "2450", lat: -30.2986, lon: 153.1094 },
    AuLocality { suburb: "Orange",         city: "Orange",        state: "NSW", postcode: "2800", lat: -33.2837, lon: 149.1007 },
    AuLocality { suburb: "Dubbo",          city: "Dubbo",         state: "NSW", postcode: "2830", lat: -32.2569, lon: 148.6011 },
    // VIC
    AuLocality { suburb: "Melbourne",      city: "Melbourne",     state: "VIC", postcode: "3000", lat: -37.8136, lon: 144.9631 },
    AuLocality { suburb: "Geelong",        city: "Geelong",       state: "VIC", postcode: "3220", lat: -38.1499, lon: 144.3617 },
    AuLocality { suburb: "Ballarat",       city: "Ballarat",      state: "VIC", postcode: "3350", lat: -37.5622, lon: 143.8503 },
    AuLocality { suburb: "Bendigo",        city: "Bendigo",       state: "VIC", postcode: "3550", lat: -36.7570, lon: 144.2794 },
    AuLocality { suburb: "Frankston",      city: "Frankston",     state: "VIC", postcode: "3199", lat: -38.1440, lon: 145.1207 },
    AuLocality { suburb: "Mildura",        city: "Mildura",       state: "VIC", postcode: "3500", lat: -34.1843, lon: 142.1620 },
    AuLocality { suburb: "Shepparton",     city: "Shepparton",    state: "VIC", postcode: "3630", lat: -36.3833, lon: 145.3999 },
    // QLD
    AuLocality { suburb: "Brisbane",       city: "Brisbane",      state: "QLD", postcode: "4000", lat: -27.4698, lon: 153.0251 },
    AuLocality { suburb: "Gold Coast",     city: "Gold Coast",    state: "QLD", postcode: "4217", lat: -28.0167, lon: 153.4000 },
    AuLocality { suburb: "Sunshine Coast", city: "Sunshine Coast",state: "QLD", postcode: "4557", lat: -26.6500, lon: 153.0667 },
    AuLocality { suburb: "Townsville",     city: "Townsville",    state: "QLD", postcode: "4810", lat: -19.2590, lon: 146.8169 },
    AuLocality { suburb: "Cairns",         city: "Cairns",        state: "QLD", postcode: "4870", lat: -16.9186, lon: 145.7781 },
    AuLocality { suburb: "Toowoomba",      city: "Toowoomba",     state: "QLD", postcode: "4350", lat: -27.5598, lon: 151.9507 },
    AuLocality { suburb: "Mackay",         city: "Mackay",        state: "QLD", postcode: "4740", lat: -21.1436, lon: 149.1878 },
    // SA
    AuLocality { suburb: "Adelaide",       city: "Adelaide",      state: "SA",  postcode: "5000", lat: -34.9285, lon: 138.6007 },
    AuLocality { suburb: "Mount Gambier",  city: "Mount Gambier", state: "SA",  postcode: "5290", lat: -37.8286, lon: 140.7826 },
    AuLocality { suburb: "Whyalla",        city: "Whyalla",       state: "SA",  postcode: "5600", lat: -33.0340, lon: 137.5832 },
    AuLocality { suburb: "Port Augusta",   city: "Port Augusta",  state: "SA",  postcode: "5700", lat: -32.4936, lon: 137.7742 },
    // WA
    AuLocality { suburb: "Perth",          city: "Perth",         state: "WA",  postcode: "6000", lat: -31.9505, lon: 115.8605 },
    AuLocality { suburb: "Fremantle",      city: "Fremantle",     state: "WA",  postcode: "6160", lat: -32.0569, lon: 115.7439 },
    AuLocality { suburb: "Bunbury",        city: "Bunbury",       state: "WA",  postcode: "6230", lat: -33.3264, lon: 115.6415 },
    AuLocality { suburb: "Geraldton",      city: "Geraldton",     state: "WA",  postcode: "6530", lat: -28.7774, lon: 114.6145 },
    AuLocality { suburb: "Kalgoorlie",     city: "Kalgoorlie",    state: "WA",  postcode: "6430", lat: -30.7490, lon: 121.4660 },
    // TAS
    AuLocality { suburb: "Hobart",         city: "Hobart",        state: "TAS", postcode: "7000", lat: -42.8821, lon: 147.3272 },
    AuLocality { suburb: "Launceston",     city: "Launceston",    state: "TAS", postcode: "7250", lat: -41.4332, lon: 147.1441 },
    AuLocality { suburb: "Devonport",      city: "Devonport",     state: "TAS", postcode: "7310", lat: -41.1775, lon: 146.3497 },
    // ACT
    AuLocality { suburb: "Canberra",       city: "Canberra",      state: "ACT", postcode: "2601", lat: -35.2809, lon: 149.1300 },
    AuLocality { suburb: "Belconnen",      city: "Canberra",      state: "ACT", postcode: "2617", lat: -35.2375, lon: 149.0608 },
    AuLocality { suburb: "Woden Valley",   city: "Canberra",      state: "ACT", postcode: "2606", lat: -35.3450, lon: 149.0850 },
    // NT
    AuLocality { suburb: "Darwin",         city: "Darwin",        state: "NT",  postcode: "0800", lat: -12.4634, lon: 130.8456 },
    AuLocality { suburb: "Alice Springs",  city: "Alice Springs", state: "NT",  postcode: "0870", lat: -23.6980, lon: 133.8807 },
    AuLocality { suburb: "Katherine",      city: "Katherine",     state: "NT",  postcode: "0850", lat: -14.4647, lon: 132.2666 },
];

/// Pick a random Australian locality using the provided RNG.
pub fn random_au_locality<R: rand::Rng>(rng: &mut R) -> &'static AuLocality {
    &AU_LOCALITIES[rng.random_range(0..AU_LOCALITIES.len())]
}

/// Pick a random Australian locality using a thread-local RNG (for contexts without an explicit RNG).
pub fn random_au_locality_thread() -> &'static AuLocality {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    &AU_LOCALITIES[rng.gen_range(0..AU_LOCALITIES.len())]
}
