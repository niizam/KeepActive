#include <stdio.h>
#include <windows.h>
#include <stdbool.h>
#include <pthread.h>
#include <stdatomic.h>
#include <unistd.h>
#include <string.h>
#include <wchar.h>
#include <tlhelp32.h> // Required for process snapshot functions

// --- Global variables ---
atomic_bool isActive = false;
wchar_t windowName[256] = L"CounterSide"; // Default/fallback window name
wchar_t processName[256] = {0};           // Executable name, initially empty
bool userSpecifiedTarget = false;         // Flag to check if -w or -e was used

// List of default process names to check for if no arguments are given
const wchar_t* defaultProcessNames[] = {
    L"CounterSide.exe",
    L"UmamusumePrettyDerby.exe",
    L"nikke.exe",
    L"GF2_Exilium.exe",
    L"P5X.exe"
};
const int numDefaultProcessNames = sizeof(defaultProcessNames) / sizeof(defaultProcessNames[0]);


// --- Helper functions for finding window by process name ---

/*
 * A data structure to pass information to the EnumWindows callback.
 * It holds the target process ID and will receive the found window handle.
 */
typedef struct {
    DWORD processId;
    HWND hwnd;
} EnumWindowsData;

/*
 * A callback function used by EnumWindows.
 * It checks if a given window belongs to the target process ID.
 *
 * @param hwnd Handle to the window being enumerated.
 * @param lParam A user-defined value, in this case a pointer to EnumWindowsData.
 * @return BOOL Returns TRUE to continue enumeration, FALSE to stop.
 */
BOOL CALLBACK EnumWindowsProc(HWND hwnd, LPARAM lParam) {
    EnumWindowsData* data = (EnumWindowsData*)lParam;
    DWORD windowProcessId;
    GetWindowThreadProcessId(hwnd, &windowProcessId);

    // We're looking for a visible window with a title that belongs to our target process
    if (windowProcessId == data->processId && IsWindowVisible(hwnd) && GetWindowTextLengthW(hwnd) > 0) {
        data->hwnd = hwnd; // Found the window
        return FALSE; // Stop enumerating
    }
    return TRUE; // Continue enumerating
}

/*
 * Gets the Process ID (PID) for a given executable name.
 *
 * @param name The wide-character string of the executable name (e.g., L"notepad.exe").
 * @return DWORD The process ID if found, otherwise 0.
 */
DWORD GetProcessIdByName(const wchar_t* name) {
    PROCESSENTRY32W entry;
    entry.dwSize = sizeof(PROCESSENTRY32W);

    // Create a snapshot of all running processes
    HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snapshot == INVALID_HANDLE_VALUE) {
        // Silently fail, as this function is called frequently.
        // fprintf(stderr, "Failed to create process snapshot.\n");
        return 0;
    }

    DWORD pid = 0;
    // Iterate through the processes in the snapshot
    if (Process32FirstW(snapshot, &entry)) {
        do {
            // Use case-insensitive comparison for the executable file name
            if (_wcsicmp(entry.szExeFile, name) == 0) {
                pid = entry.th32ProcessID;
                break;
            }
        } while (Process32NextW(snapshot, &entry));
    }

    CloseHandle(snapshot); // Always clean up the snapshot handle
    return pid;
}

// --- Main application logic ---

/*
 * The main thread function that keeps the target window active.
 * It periodically finds the window and sends it an activation message.
 */
void* keepActive(void* arg) {
    while (atomic_load(&isActive)) {
        HWND hwnd = NULL;

        // If a user specified a target, use the original logic
        if (userSpecifiedTarget) {
            // Priority 1: Try to find by process name if specified
            if (processName[0] != L'\0') {
                DWORD pid = GetProcessIdByName(processName);
                if (pid != 0) {
                    EnumWindowsData data = { .processId = pid, .hwnd = NULL };
                    EnumWindows(EnumWindowsProc, (LPARAM)&data);
                    hwnd = data.hwnd;
                }
            }
            // Priority 2: Fall back to finding by window name
            if (hwnd == NULL) {
                hwnd = FindWindowW(NULL, windowName);
            }
        }
        // Otherwise, use the new default fallback logic
        else {
            // Priority 1: Iterate through the default process list
            for (int i = 0; i < numDefaultProcessNames; i++) {
                DWORD pid = GetProcessIdByName(defaultProcessNames[i]);
                if (pid != 0) {
                    EnumWindowsData data = { .processId = pid, .hwnd = NULL };
                    EnumWindows(EnumWindowsProc, (LPARAM)&data);
                    hwnd = data.hwnd;
                    if (hwnd != NULL) {
                        break; // Found one, stop searching
                    }
                }
            }
            // Priority 2: Fall back to the default window name if list fails
            if (hwnd == NULL) {
                hwnd = FindWindowW(NULL, windowName);
            }
        }

        // If a window handle was found, send the activation message
        if (hwnd != NULL) {
            SendMessageW(hwnd, WM_ACTIVATE, WA_CLICKACTIVE, 0);
        }

        // Sleep for 100ms before the next check
        usleep(100000);
    }
    return NULL;
}

/*
 * The main entry point of the application.
 */
int main(int argc, char* argv[]) {
    // Parse command-line arguments for window title (-w) or executable name (-e)
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "-w") == 0 && i + 1 < argc) {
            // Convert multi-byte argument to a wide-character string for the window name
            mbstowcs(windowName, argv[i + 1], strlen(argv[i + 1]) + 1);
            userSpecifiedTarget = true;
            i++; // Increment i to skip the argument's value
        } else if (strcmp(argv[i], "-e") == 0 && i + 1 < argc) {
            // Convert multi-byte argument to a wide-character string for the process name
            mbstowcs(processName, argv[i + 1], strlen(argv[i + 1]) + 1);
            userSpecifiedTarget = true;
            i++; // Increment i to skip the argument's value
        }
    }

    printf("Keep Active - C CLI Version\n");
    if (userSpecifiedTarget) {
        if (processName[0] != L'\0') {
            printf("Target Process: %ls\n", processName);
        }
        // Only show window name if it was explicitly set or is the only target
        if (wcscmp(windowName, L"CounterSide") != 0 || processName[0] == L'\0') {
            printf("Target/Fallback Window: %ls\n", windowName);
        }
    } else {
        printf("No target specified. Using default search order:\n");
        printf("1. Default Processes:\n");
        for (int i = 0; i < numDefaultProcessNames; i++) {
            printf("   - %ls\n", defaultProcessNames[i]);
        }
        printf("2. Fallback Window Title: %ls\n", windowName);
    }

    printf("----------------------------------------\n");
    printf("Type '1' to turn on, '0' to turn off, 'q' to quit\n");

    pthread_t keepActiveThread;
    bool threadRunning = false;
    char input;

    while (true) {
        if (scanf(" %c", &input) != 1) {
            // On invalid input, clear the buffer and try again
            while (getchar() != '\n');
            continue;
        };

        if (input == '1' && !atomic_load(&isActive)) {
            atomic_store(&isActive, true);
            if (pthread_create(&keepActiveThread, NULL, keepActive, NULL) != 0) {
                fprintf(stderr, "Error creating thread\n");
                return 1;
            }
            threadRunning = true;
            printf("Running\n");
        } else if (input == '0' && atomic_load(&isActive)) {
            atomic_store(&isActive, false);
            if (threadRunning) {
                pthread_join(keepActiveThread, NULL);
                threadRunning = false;
            }
            printf("Not Running\n");
        } else if (input == 'q') {
            // Ensure the thread is stopped before quitting
            if (atomic_load(&isActive)) {
                atomic_store(&isActive, false);
                if (threadRunning) {
                    pthread_join(keepActiveThread, NULL);
                }
            }
            break; // Exit the main loop
        }
    }

    printf("Exiting program\n");
    return 0;
}
