// Reference solution: menu with switch and an enum (tests/assignments/bank_menu).
// Each `std::cin >>` stands for an *ask* block; with one valid answer per
// line, both print the same.

#include <iostream>

enum class Action { Deposit, Withdraw, ShowBalance, Quit, Unknown };

// The action for a menu choice.
Action to_action(int choice) {
    switch (choice) {
        case 1: {
            return Action::Deposit;
        }
        case 2: {
            return Action::Withdraw;
        }
        case 3: {
            return Action::ShowBalance;
        }
        case 4: {
            return Action::Quit;
        }
        default: {
            return Action::Unknown;
        }
    }
}

int main() {
    int balance = 0;
    Action action = Action::Unknown;
    do {
        std::cout << '\n';
        std::cout << "1) Deposit" << '\n';
        std::cout << "2) Withdraw" << '\n';
        std::cout << "3) Show balance" << '\n';
        std::cout << "4) Quit" << '\n';
        int choice = 0;
        std::cout << "Choice: ";
        std::cin >> choice;
        action = to_action(choice);
        switch (action) {
            case Action::Deposit: {
                int amount = 0;
                std::cout << "Amount: ";
                std::cin >> amount;
                if (amount <= 0) {
                    std::cout << "The amount must be more than 0." << '\n';
                } else {
                    balance += amount;
                    std::cout << "Deposited " << amount << "." << '\n';
                }
                break;
            }
            case Action::Withdraw: {
                int amount = 0;
                std::cout << "Amount: ";
                std::cin >> amount;
                if (amount <= 0) {
                    std::cout << "The amount must be more than 0." << '\n';
                } else if (amount > balance) {
                    std::cout << "Not enough money: the balance is " << balance << "." << '\n';
                } else {
                    balance -= amount;
                    std::cout << "Withdrew " << amount << "." << '\n';
                }
                break;
            }
            case Action::ShowBalance: {
                std::cout << "Balance: " << balance << '\n';
                break;
            }
            case Action::Quit: {
                std::cout << "Goodbye!" << '\n';
                break;
            }
            case Action::Unknown: {
                std::cout << "Please choose 1, 2, 3 or 4." << '\n';
                break;
            }
        }
    } while (action != Action::Quit);
    return 0;
}
