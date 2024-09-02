use mac_address::MacAddress;
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

const MAC_ADDRESS_BITS: usize = 48;
const COUNTER_BITS: usize = 64 - MAC_ADDRESS_BITS;

static GENERATOR: LazyLock<Mutex<Generator>> = LazyLock::new(|| { Mutex::new(Generator::new()) });
struct Generator {
    mac: u64,
    counter: u64,
    last_time: u128,
}
impl Generator {
    fn new() -> Self {
        let mac_addr: MacAddress;
        match mac_address::get_mac_address() {
            Ok(Some(m)) => { mac_addr = m; }
            Ok(None) => {
                panic!("no mac address");
            }
            Err(e) => {
                panic!("get mac address error: {:?}", e);
            }
        }

        let mut mac: u64 = 0;
        for b in mac_addr.bytes() {
            mac = (mac << 8) | (b as u64);
        }

        Self {
            mac,
            counter: 0,
            last_time: 0,
        }
    }
}

pub fn gen_id() -> u128 {
    let mut generator = GENERATOR.lock().unwrap();
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos();
    if generator.last_time < now {
        generator.last_time = now;
        generator.counter = 0;
    } else {
        generator.counter = (generator.counter + 1) % (1 << COUNTER_BITS);
    }
    (now << 64) | ((generator.counter as u128) << MAC_ADDRESS_BITS) | (generator.mac as u128)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::{Arc, Barrier};
    use std::thread;

    #[test]
    fn test_gen_id() {
        let id = gen_id();
        assert!(id > 0);
    }

    #[test]
    fn test_gen_id_concurrent() {
        let barrier = Arc::new(Barrier::new(1000));
        let mut threads = Vec::new();
        for _ in 0..1000 {
            let barrier = barrier.clone();
            threads.push(thread::spawn(move || {
                barrier.wait();
                let id = gen_id();
                assert!(id > 0);
            }));
        }
        for t in threads {
            t.join().unwrap();
        }
    }

    #[test]
    fn test_gen_id_concurrent_unique() {
        let barrier = Arc::new(Barrier::new(1000));
        let mut threads = Vec::new();
        let ids = Arc::new(Mutex::new(HashSet::new()));
        for _ in 0..1000 {
            let barrier = barrier.clone();
            let ids = ids.clone();
            threads.push(thread::spawn(move || {
                barrier.wait();
                let id = gen_id();
                assert!(id > 0);
                let mut ids = ids.lock().unwrap();
                assert!(ids.insert(id));
            }));
        }
        for t in threads {
            t.join().unwrap();
        }
    }
}
