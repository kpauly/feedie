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
When the user toggles between "Present" and "Empty"
Then each thumbnail shows the filename above, the predicted species + confidence below, and lazy-loads as needed
And the user can select frames using click, Ctrl-click, Shift-range, or Ctrl-A for later context actions
And the gallery shows 100 cards per page with pager controls at the top/bottom and keyboard navigation (arrows, Home/End, Page Up/Down) to move within/among pages, including Shift + navigation to extend selection from the anchor

Scenario 5: Opt-in Roboflow upload
Given the Instellingen panel is open
And the user enables "Help de herkenning te verbeteren" and leaves the dataset name at "voederhuiscamera"
When the user recategorizes one or more images via the context menu
Then a background upload is triggered and the Roboflow dataset receives the images with the selected label without blocking the UI

Scenario 6: Create a new category via "Nieuw..."
Given the user opens the context menu on one or more selected thumbnails
And the menu shows the "Nieuw... >" entry
When the user types a new label name, presses Enter (or clicks OK), and the upload toggle is enabled
Then the frames immediately get the manual label, move to the Present tab, and the image+label is uploaded to Roboflow in the background

Scenario 7: Export selected thumbnails to labeled folders
Given one or more thumbnails with possibly different species are selected
And the context menu is opened on the selection
When the user clicks "Export", picks (or creates) a destination folder, and confirms "Save"
Then the app creates subfolders named after each category label under the chosen folder
And it copies every selected image into the matching subfolder with filename `<label>_<originalfilename>.jpg`

Scenario 8: Configure batch export via Export tab
Given the user has completed a scan and opens the Export tab
And "Export photos with Present species" and "Export identification results as CSV" are checked (others optional)
When the user clicks "Export", chooses a destination folder, and pastes the Google Maps coordinates in the prompt
Then the app creates subfolders per selected option (per species, "Uncertain", "Background")
And copies the relevant images as `<label>_<originalfilename>.jpg` into each subfolder
And writes `voederhuiscamera_yymmddhhmm.csv` in the export root with columns `date,time,scientific name,lat,lng,path` filled using the entered coordinates and the new file locations
