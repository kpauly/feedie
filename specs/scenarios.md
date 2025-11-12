# Scenarios

Scenario 1: Empty Folder
Given an empty folder is selected
And the pre-scan summary shows "Images in folder: 0"
When the user runs "Scan"
Then the UI shows "No images found"
And CSV export is disabled

Scenario 2: Present filter hides empty frames
Given a folder with mixed empty and non-empty frames
And after scanning, the default view is "Present"
When "Present" is selected
Then only frames with birds are shown

Scenario 2b: Switch to empty frames view
Given a folder with mixed empty and non-empty frames
And after scanning, the default view is "Present"
When the user switches the view to "Empty"
Then only frames without birds are shown

Scenario 3: Unknown species abstention
Given a crop with low similarity to any reference
When classified via k-NN
Then the label is "Unknown"
And confidence is below the threshold

Scenario 4: Thumbnail review & selection
Given a completed scan with both present and empty frames
When the user toggles between "Aanwezig" and "Leeg"
Then each thumbnail shows the filename above, the predicted species + confidence below, and lazy-loads as needed
And the user can select frames using click, Ctrl-click, Shift-range, or Ctrl-A for later context actions
