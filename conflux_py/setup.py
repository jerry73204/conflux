"""Setup script for conflux_py package (ROS2 ament_python compatibility)."""

from setuptools import find_packages, setup

package_name = "conflux_py"

setup(
    name=package_name,
    version="0.2.0",
    packages=find_packages(exclude=["test"]),
    data_files=[
        ("share/ament_index/resource_index/packages", [f"resource/{package_name}"]),
        (f"share/{package_name}", ["package.xml"]),
    ],
    install_requires=["setuptools"],
    zip_safe=False,
    maintainer="Aeon",
    maintainer_email="aeon@example.com",
    description="Python library for multi-stream message synchronization",
    license="MIT OR Apache-2.0",
    tests_require=["pytest"],
    entry_points={
        "console_scripts": [],
    },
)
