#[macro_use]
extern crate serde;
use candid::{Decode, Encode};
use ic_cdk::api::time;
use ic_cdk::caller;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{BoundedStorable, Cell, DefaultMemoryImpl, StableBTreeMap, Storable};
use std::{borrow::Cow, cell::RefCell};

type Memory = VirtualMemory<DefaultMemoryImpl>;
type IdCell = Cell<u64, Memory>;

#[derive(candid::CandidType, Clone, Serialize, Deserialize, Default)]
struct Employee {
    id: u64,
    name: String,
    email: String,
    employer_id: String,
    rating: Option<String>,
    transferable: bool,
    created_at: u64,
    updated_at: Option<u64>,
}

impl Storable for Employee {
    fn to_bytes(&self) -> std::borrow::Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: std::borrow::Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
}

impl BoundedStorable for Employee {
    const MAX_SIZE: u32 = 1024;
    const IS_FIXED_SIZE: bool = false;
}

thread_local! {
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(
        MemoryManager::init(DefaultMemoryImpl::default())
    );

    static ID_COUNTER: RefCell<IdCell> = RefCell::new(
        IdCell::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0))), 0)
            .expect("Cannot create a counter")
    );

    static STORAGE: RefCell<StableBTreeMap<u64, Employee, Memory>> =
        RefCell::new(StableBTreeMap::init(
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1)))
    ));
}

#[derive(candid::CandidType, Serialize, Deserialize, Default)]
struct EmployeePayload {
    name: String,
    email: String,
}

#[derive(candid::CandidType, Serialize, Deserialize, Default)]
struct RatingPayload {
    employee_id: u64,
    rating: String,
}

/*
    Get a list of all employees under your employ
*/

#[ic_cdk::query]
fn get_employees() -> Result<Vec<Employee>, Error> {
    let employees_map: Vec<(u64, Employee)> =
        STORAGE.with(|sevice| sevice.borrow().iter().collect());

    let employees_vec: Vec<Employee> = employees_map
        .into_iter()
        .map(|(_, employee)| employee)
        .collect();

    if !employees_vec.is_empty() {
        let mut employees: Vec<Employee> = Vec::new();

        for employee in employees_vec {
            if caller().to_string() == employee.employer_id {
                employees.push(employee);
            };
        }

        if !employees.is_empty() {
            Ok(employees)
        } else {
            Err(Error::NotFound {
                msg: ("No employee Records found".to_string()),
            })
        }
    } else {
        Err(Error::NotFound {
            msg: ("No employee Records found".to_string()),
        })
    }
}

/*
    Get a single employee by their ID
*/

#[ic_cdk::query]
fn get_employee(id: u64) -> Result<Employee, Error> {
    match _get_employee(&id) {
        Some(employee) => {
            if employee.employer_id == caller().to_string() {
                Ok(employee)
            } else {
                Err(Error::NotFound {
                    msg: ("Cannot view an employee you did not employ".to_string()),
                })
            }
        }

        None => Err(Error::NotFound {
            msg: format!("No employee with id={} found", id),
        }),
    }
}

/*
    Create an employee record
*/

#[ic_cdk::update]
fn create_employee(payload: EmployeePayload) -> Option<Employee> {
    let id = ID_COUNTER
        .with(|counter| {
            let current_value = *counter.borrow().get();
            counter.borrow_mut().set(current_value + 1)
        })
        .expect("cannot increment id counter");

    let employee = Employee {
        id,
        name: payload.name,
        email: payload.email,
        employer_id: caller().to_string(),
        rating: None,
        transferable: false,
        created_at: time(),
        updated_at: Some(time()),
    };

    do_insert(&employee);
    Some(employee)
}

/*
    Give an employee a rating
*/

#[ic_cdk::update]
fn set_rating(payload: RatingPayload) -> Result<Employee, Error> {
    let ratings = [
        "excellent".to_string(),
        "good".to_string(),
        "average".to_string(),
        "satisfactory".to_string(),
        "poor".to_string(),
    ];

    match _get_employee(&payload.employee_id) {
        Some(mut employee) => {
            if employee.employer_id == caller().to_string() {
                if !ratings.contains(&payload.rating) {
                    Err(Error::InvalidType {
                        msg: format!("Invalid rating type! Accepted values are: {:?}", ratings),
                    })
                } else {
                    employee.rating = Some(payload.rating);
                    employee.updated_at = Some(time());
                    do_insert(&employee);
                    Ok(employee)
                }
            } else {
                Err(Error::NotFound {
                    msg: ("Cannot rate an employee you did not employ".to_string()),
                })
            }
        }

        None => Err(Error::NotFound {
            msg: format!("No employee with id={} found", payload.employee_id),
        }),
    }
}

/*
    toggle your employee's transferable status
*/

#[ic_cdk::update]
fn toggle_transferable(employee_id: u64) -> Result<String, Error> {
    match _get_employee(&employee_id) {
        Some(mut employee) => {
            if employee.employer_id == caller().to_string() {
                if employee.transferable == true {
                    employee.transferable = false;
                } else {
                    employee.transferable = true;
                };
                employee.updated_at = Some(time());
                do_insert(&employee);
                Ok(format!(
                    "Employee with ID: {} has transferable toggled to: {}",
                    employee_id, employee.transferable
                ))
            } else {
                Err(Error::Forbidden {
                    msg: format!("Cannot alter an employee tranferable status you did employ"),
                })
            }
        }
        None => Err(Error::NotFound {
            msg: format!("No employee with id={} found", employee_id),
        }),
    }
}

/*
    transfer an employee to another employer
*/
#[ic_cdk::update]
fn add_employee(employee_id: u64) -> Result<String, Error> {
    match _get_employee(&employee_id) {
        Some(mut employee) => {
            if employee.transferable == true {
                employee.employer_id = caller().to_string();
                employee.updated_at = Some(time());
                do_insert(&employee);

                Ok(format!(
                    "Employee with ID: {} has been added to your employ",
                    employee_id
                ))
            } else {
                Err(Error::Forbidden {
                    msg: format!("Employee with ID: {} is not transferable", employee_id),
                })
            }
        }
        None => Err(Error::NotFound {
            msg: format!("No employee with id={} found", employee_id),
        }),
    }
}

/*
    Delete your employee
*/
#[ic_cdk::update]
fn delete_employee(employee_id: u64) -> Result<String, Error> {
    match STORAGE.with(|service| service.borrow_mut().remove(&employee_id)) {
        Some(employee) => Ok(format!(
            "Employee with ID: {} has been deleted",
            employee.id
        )),
        None => Err(Error::NotFound {
            msg: format!("No employee with id={} found", employee_id),
        }),
    }
}

/*
    helper functions and enums
*/

fn _get_employee(id: &u64) -> Option<Employee> {
    STORAGE.with(|s| s.borrow().get(id))
}

fn do_insert(employee: &Employee) {
    STORAGE.with(|service| service.borrow_mut().insert(employee.id, employee.clone()));
}

#[derive(candid::CandidType, Deserialize, Serialize)]
enum Error {
    NotFound { msg: String },
    InvalidType { msg: String },
    Forbidden { msg: String },
}

ic_cdk::export_candid!();
