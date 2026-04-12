Next things to try:

1. Let's work on cleaning up the schemas. I'm still not sure if site and dataset names are being correctly qualified with their projects.

2. Projects in loco-cards aren't creating correctly. They didn't create a default dataset or site.

Does all this make sense. Please push back on this if you see any problems with this separation, or have any caviats to add for me to think through.

3. Maybe we should be able to edit the labels/descriptions of projects in the cards app.

4. Enforce create-only properties and write tests.

5. It looks like users aren't stored in the data-lake. I'm wonder if we could adjust so that those are actually stored in the lake.

6. It looks like the auth functionality isn't using the x-site id header like the other functionality is. We should switch to doing that.

7. For loco-lake, let's put the individual adapters into an "adapters" folder.
